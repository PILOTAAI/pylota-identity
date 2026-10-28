//! Reference extraction and normalisation ([Inbound › Reference extraction], [Search § 3.5]): amounts,
//! phone numbers, emails, domains, dates, invoice and order numbers (pack `core`), UK plates and PCNs
//! (pack `uk_vehicle`) and tenant patterns (`custom_refs`). Ingest stores the normalised values; search
//! runs the same normalisers over `ref:` values and ref-like terms, so `AB12 CDE` finds `AB12CDE` (F5).
//!
//! [Inbound › Reference extraction]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Search § 3.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

mod packs;
pub mod phone;

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use regex::{Regex, RegexBuilder};

/// At most this many refs of one kind per message.
pub const MAX_PER_KIND: usize = 50;
/// At most this many refs per message.
pub const MAX_PER_MESSAGE: usize = 200;
/// Longer values are dropped, never truncated (a cut value would match the wrong mail).
pub const MAX_VALUE_CHARS: usize = 64;
/// `policy.search.custom_refs` holds at most this many patterns.
pub const MAX_CUSTOM_REFS: usize = 20;
/// Compiled size cap of a custom pattern, bytes.
pub const CUSTOM_SIZE_LIMIT: usize = 64 * 1024;
/// `RefMatch.candidates` holds at most this many values.
pub const MAX_CANDIDATES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    Amount,
    Phone,
    Email,
    Domain,
    Date,
    Invoice,
    Order,
    UkPlate,
    Pcn,
    /// `custom:{name}`
    Custom(String),
    /// The fallback form of a `ref:` value (upper case, spaces, dots and hyphens removed). Search only;
    /// extraction never produces it.
    Raw,
}

impl RefKind {
    /// The stored `refs.kind`.
    pub fn as_str(&self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::Amount => "amount",
            Self::Phone => "phone",
            Self::Email => "email",
            Self::Domain => "domain",
            Self::Date => "date",
            Self::Invoice => "invoice",
            Self::Order => "order",
            Self::UkPlate => "uk_plate",
            Self::Pcn => "pcn",
            Self::Raw => "raw",
            Self::Custom(name) => return Cow::Owned(format!("custom:{name}")),
        })
    }
}

/// `policy.search.refs_packs`. `Core` is always on, listed or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    Core,
    UkVehicle,
}

impl Pack {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "core" => Some(Self::Core),
            "uk_vehicle" => Some(Self::UkVehicle),
            _ => None,
        }
    }
}

/// `custom_refs[].normalise`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalise {
    Upper,
    Lower,
    None,
}

/// One compiled `custom_refs` entry.
#[derive(Clone, Debug)]
pub struct CustomRef {
    pub name: String,
    pub normalise: Normalise,
    regex: Regex,
}

impl CustomRef {
    /// Compiles `pattern` with the `regex` crate (linear time, no back-references) under a 64 KB size
    /// limit. An error means the policy is refused when it is saved.
    pub fn new(name: &str, pattern: &str, normalise: Normalise) -> Result<Self, regex::Error> {
        let regex = RegexBuilder::new(pattern)
            .size_limit(CUSTOM_SIZE_LIMIT)
            .build()?;
        Ok(Self {
            name: name.to_owned(),
            normalise,
            regex,
        })
    }

    /// Each match as (start, end, normalised value): the whole match, or capture group 1 if the
    /// pattern has one, normalised and trimmed.
    fn scan(&self, text: &str, out: &mut Vec<packs::Hit>) {
        let group = self.regex.captures_len() > 1;
        for caps in self.regex.captures_iter(text) {
            let Some(whole) = caps.get(0) else { continue };
            let Some(m) = (if group { caps.get(1) } else { Some(whole) }) else {
                continue;
            };
            let v = match self.normalise {
                Normalise::Upper => m.as_str().to_uppercase(),
                Normalise::Lower => m.as_str().to_lowercase(),
                Normalise::None => m.as_str().to_owned(),
            };
            let v = v.trim();
            if !v.is_empty() {
                out.push(packs::Hit {
                    kind: RefKind::Custom(self.name.clone()),
                    start: whole.start(),
                    end: whole.end(),
                    value: v.to_owned(),
                });
            }
        }
    }
}

/// Where a ref was found.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RefSource {
    Subject,
    Body,
    Attachment { attachment_id: String, page: u32 },
}

impl fmt::Display for RefSource {
    /// The stored `refs.source`: `subject`, `body` or `att:{attachment_id}:{page}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Subject => f.write_str("subject"),
            Self::Body => f.write_str("body"),
            Self::Attachment {
                attachment_id,
                page,
            } => write!(f, "att:{attachment_id}:{page}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub kind: RefKind,
    pub value: String,
    pub source: RefSource,
}

/// A normalised value without a source (`RefMatch.candidates`, `CompiledQuery.ref_like`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefValue {
    pub kind: RefKind,
    pub value: String,
}

/// The enabled normalisers for one tenant.
struct Ctx<'a> {
    uk_vehicle: bool,
    custom: &'a [CustomRef],
    country: &'static phone::Country,
    /// `D/M/YYYY` reads day first (tenant time zone under `Europe/`).
    day_first: bool,
}

impl<'a> Ctx<'a> {
    fn new(packs: &[Pack], custom: &'a [CustomRef], tz: &str) -> Self {
        Self {
            uk_vehicle: packs.contains(&Pack::UkVehicle),
            custom: &custom[..custom.len().min(MAX_CUSTOM_REFS)],
            country: phone::country_for_tz(tz),
            day_first: tz.starts_with("Europe/"),
        }
    }

    fn scan(&self, text: &str) -> Vec<packs::Hit> {
        let mut out = Vec::new();
        packs::scan_core(text, self, &mut out);
        if self.uk_vehicle {
            packs::scan_uk_vehicle(text, &mut out);
        }
        for c in self.custom {
            c.scan(text, &mut out);
        }
        out
    }
}

/// Bounded collection: de-duplicated, at most 50 per kind and 200 in all, values ≤ 64 characters.
#[derive(Default)]
struct Collector {
    refs: Vec<Ref>,
    seen: HashSet<(RefKind, String, String)>,
}

impl Collector {
    fn add(&mut self, hits: Vec<packs::Hit>, source: &RefSource) {
        let src = source.to_string();
        for h in hits {
            if self.refs.len() >= MAX_PER_MESSAGE {
                return;
            }
            if h.value.chars().count() > MAX_VALUE_CHARS
                || self.refs.iter().filter(|r| r.kind == h.kind).count() >= MAX_PER_KIND
                || !self
                    .seen
                    .insert((h.kind.clone(), h.value.clone(), src.clone()))
            {
                continue;
            }
            self.refs.push(Ref {
                kind: h.kind,
                value: h.value,
                source: source.clone(),
            });
        }
    }
}

/// Refs of one message, from its subject and `extracted_text` (never the quoted history), with the
/// tenant's packs, custom patterns and time zone (`tenants.timezone`, for phone and date rules).
pub fn extract(
    subject: &str,
    extracted_text: &str,
    packs: &[Pack],
    custom: &[CustomRef],
    tz: &str,
) -> Vec<Ref> {
    let ctx = Ctx::new(packs, custom, tz);
    let mut c = Collector::default();
    c.add(ctx.scan(subject), &RefSource::Subject);
    c.add(ctx.scan(extracted_text), &RefSource::Body);
    c.refs
}

/// Refs of one page of extracted attachment text, under the same caps.
pub fn extract_attachment(
    text: &str,
    attachment_id: &str,
    page: u32,
    packs: &[Pack],
    custom: &[CustomRef],
    tz: &str,
) -> Vec<Ref> {
    let ctx = Ctx::new(packs, custom, tz);
    let mut c = Collector::default();
    let source = RefSource::Attachment {
        attachment_id: attachment_id.to_owned(),
        page,
    };
    c.add(ctx.scan(text), &source);
    c.refs
}

/// The normalised form from every enabled normaliser that accepts all of `raw` (after trimming), in
/// pack order, de-duplicated by value.
pub fn normalise(raw: &str, packs: &[Pack], custom: &[CustomRef], tz: &str) -> Vec<RefValue> {
    let raw = raw.trim();
    let ctx = Ctx::new(packs, custom, tz);
    let mut out: Vec<RefValue> = Vec::new();
    let mut push = |kind: RefKind, value: String| {
        if value.chars().count() <= MAX_VALUE_CHARS && !out.iter().any(|v| v.value == value) {
            out.push(RefValue { kind, value });
        }
    };
    for h in ctx.scan(raw) {
        if h.start == 0 && h.end == raw.len() {
            push(h.kind, h.value);
        }
    }
    // A bare domain is a `domain` ref too (ingest only sees domains inside emails and URLs).
    if let Some(d) = packs::bare_domain(raw) {
        push(RefKind::Domain, d);
    }
    out
}

/// `RefMatch.candidates` for `ref:<raw>`: [`normalise`] plus the fallback form (upper case with spaces,
/// dots and hyphens removed), de-duplicated, at most 8.
pub fn candidates(raw: &str, packs: &[Pack], custom: &[CustomRef], tz: &str) -> Vec<RefValue> {
    let mut out = normalise(raw, packs, custom, tz);
    let fallback: String = raw
        .trim()
        .to_uppercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '.' | '-'))
        .collect();
    if !fallback.is_empty()
        && fallback.chars().count() <= MAX_VALUE_CHARS
        && !out.iter().any(|v| v.value == fallback)
    {
        out.push(RefValue {
            kind: RefKind::Raw,
            value: fallback,
        });
    }
    out.truncate(MAX_CANDIDATES);
    out
}

/// The spaced display form of a normalised plate for the FTS `refs` column (`AB12CDE` → `AB12 CDE`):
/// a space before the last three letters (current and prefix formats) or after the first three
/// (suffix format).
pub fn plate_display(value: &str) -> Option<String> {
    let b = value.as_bytes();
    if !(5..=7).contains(&b.len()) || !b.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    let tail_letters = b[b.len() - 3..].iter().all(u8::is_ascii_uppercase);
    let at = if tail_letters { b.len() - 3 } else { 3 };
    Some(format!("{} {}", &value[..at], &value[at..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const LONDON: &str = "Europe/London";
    const UK: &[Pack] = &[Pack::Core, Pack::UkVehicle];

    fn values(text: &str, packs: &[Pack], tz: &str, kind: RefKind) -> Vec<String> {
        extract("", text, packs, &[], tz)
            .into_iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.value)
            .collect()
    }

    fn norm(raw: &str, kind: RefKind) -> Option<String> {
        normalise(raw, UK, &[], LONDON)
            .into_iter()
            .find(|v| v.kind == kind)
            .map(|v| v.value)
    }

    #[test]
    fn f5_plates_with_or_without_spaces() {
        for raw in ["AB12CDE", "ab12 cde", "AB12  CDE", "AB12-CDE", "Ab12cDe"] {
            assert_eq!(
                norm(raw, RefKind::UkPlate).as_deref(),
                Some("AB12CDE"),
                "{raw}"
            );
        }
        // Prefix and suffix formats.
        assert_eq!(
            norm("A123 BCD", RefKind::UkPlate).as_deref(),
            Some("A123BCD")
        );
        assert_eq!(
            norm("abc 123d", RefKind::UkPlate).as_deref(),
            Some("ABC123D")
        );
        // In text, on word boundaries only.
        assert_eq!(
            values(
                "Car AB12 CDE was seen, not XAB12CDEF",
                UK,
                LONDON,
                RefKind::UkPlate
            ),
            ["AB12CDE"]
        );
        // The pack is opt-in.
        assert!(values("AB12 CDE", &[Pack::Core], LONDON, RefKind::UkPlate).is_empty());
        assert_eq!(plate_display("AB12CDE").as_deref(), Some("AB12 CDE"));
        assert_eq!(plate_display("ABC123D").as_deref(), Some("ABC 123D"));
        // `ref:` candidates agree whatever the spacing.
        let c = |raw| candidates(raw, UK, &[], LONDON);
        assert_eq!(c("AB12 CDE"), c("AB12CDE"));
    }

    proptest! {
        #[test]
        fn f5_plate_normalisation_idempotent(
            a in "[A-Za-z]{2}", d in "[0-9]{2}", b in "[A-Za-z]{3}", sep in "[ -]{0,3}"
        ) {
            let raw = format!("{a}{d}{sep}{b}");
            let v = norm(&raw, RefKind::UkPlate).expect("a current-format plate");
            prop_assert_eq!(&v, &format!("{a}{d}{b}").to_uppercase());
            prop_assert_eq!(norm(&v, RefKind::UkPlate), Some(v.clone()));
            let spaced = plate_display(&v).unwrap();
            prop_assert_eq!(norm(&spaced, RefKind::UkPlate), Some(v));
        }
    }

    #[test]
    fn f5_amounts() {
        let a = |text| values(text, &[], LONDON, RefKind::Amount);
        assert_eq!(a("Total £412.80 due"), ["GBP:412.80"]);
        assert_eq!(a("Total 412.80 GBP due"), ["GBP:412.80"]);
        assert_eq!(a("Betrag 412,80 €"), ["EUR:412.80"]);
        assert_eq!(a("EUR 412,80"), ["EUR:412.80"]);
        assert_eq!(a("USD 1,200"), ["USD:1200.00"]);
        assert_eq!(a("$1,200.5 paid"), ["USD:1200.50"]);
        assert_eq!(a("gbp 7"), ["GBP:7.00"]);
        assert_eq!(a("order 20000 £5.00"), ["GBP:5.00"]);
        // A comma is a decimal separator only for euro amounts of the form \d+,\d{2}.
        assert_eq!(a("£412,80"), Vec::<String>::new());
        assert_eq!(a("1.200,50 €"), Vec::<String>::new());
        // No currency, no amount; a code inside a word is not a currency.
        assert!(a("412.80 and EURO 5").is_empty());
        assert_eq!(
            norm("£412.80", RefKind::Amount),
            norm("412.80 GBP", RefKind::Amount)
        );
    }

    #[test]
    fn f5_phones() {
        let p = |text, tz| values(text, &[], tz, RefKind::Phone);
        assert_eq!(p("Call +44 7700 900123 today", LONDON), ["+447700900123"]);
        assert_eq!(p("Call 0044 (0)7700-900.123", LONDON), ["+447700900123"]);
        assert_eq!(p("Call 07700 900123", LONDON), ["+447700900123"]);
        assert_eq!(p("Call (020) 7946 0000", LONDON), ["+442079460000"]);
        // National format follows the tenant's country; an unknown zone falls back to GB.
        assert_eq!(
            p("Call (212) 555-0123", "America/New_York"),
            ["+12125550123"]
        );
        assert_eq!(
            p("Call 1-212-555-0123", "America/New_York"),
            ["+12125550123"]
        );
        assert_eq!(p("Call 07700 900123", "Etc/UTC"), ["+447700900123"]);
        assert_eq!(
            p("Appelez le 01 23 45 67 89", "Europe/Paris"),
            ["+33123456789"]
        );
        // Too few or too many digits, or digits running on, are not phones.
        assert!(p("+44 123", LONDON).is_empty());
        assert!(p("+1234567890123456", LONDON).is_empty());
        assert!(p("ref 0770090012399", LONDON).is_empty());
        assert_eq!(
            norm("+44 7700 900123", RefKind::Phone),
            norm("07700900123", RefKind::Phone)
        );
    }

    #[test]
    fn f5_dates() {
        let d = |text, tz| values(text, &[], tz, RefKind::Date);
        assert_eq!(d("due 2026-10-27", LONDON), ["2026-10-27"]);
        assert_eq!(d("due 3/4/2026", LONDON), ["2026-04-03"]);
        assert_eq!(d("due 3.4.2026", LONDON), ["2026-04-03"]);
        assert_eq!(d("due 3/4/2026", "America/New_York"), ["2026-03-04"]);
        assert_eq!(d("due 13/4/2026", "America/New_York"), ["2026-04-13"]);
        assert_eq!(d("due 27 October 2026", LONDON), ["2026-10-27"]);
        assert_eq!(d("due 7 Oct 2026", LONDON), ["2026-10-07"]);
        // Impossible dates and mixed separators are not dates.
        assert!(d("2026-02-30 and 3/4.2026 and 31/31/2026", LONDON).is_empty());
    }

    #[test]
    fn f5_invoice_and_order() {
        let inv = |text| values(text, &[], LONDON, RefKind::Invoice);
        assert_eq!(inv("Invoice 88213 attached"), ["88213"]);
        assert_eq!(inv("see INV-88213"), ["88213"]);
        assert_eq!(inv("Rechnung Nr. 2026/0042"), ["2026/0042"]);
        assert_eq!(inv("facture no: ab-1234"), ["AB-1234"]);
        assert_eq!(inv("Invoice #A7731"), ["A7731"]);
        // A value with no digit is a word, not a number.
        assert!(inv("Invoice attached").is_empty());
        let ord = |text| values(text, &[], LONDON, RefKind::Order);
        assert_eq!(ord("Your order 4521-99 shipped"), ["4521-99"]);
        assert_eq!(ord("purchase order: PO77812"), ["PO77812"]);
        assert_eq!(ord("PO-12345"), ["12345"]);
        assert_eq!(ord("Bestellung #ab12cd"), ["AB12CD"]);
        assert!(ord("PO Box 12").is_empty());
    }

    #[test]
    fn f5_every_pack_kind() {
        let text = "From Jo.Rivera@Brightwell.Example: see https://portal.shop.example.co.uk/x and \
                    bücher.example, PCN WK12345678 (penalty charge ref QX9912345), plate AB12 CDE.";
        let refs = extract("Invoice 88213", text, UK, &[], LONDON);
        let has = |k: RefKind, v: &str| refs.iter().any(|r| r.kind == k && r.value == v);
        assert!(has(RefKind::Invoice, "88213"));
        assert!(refs.iter().any(|r| r.source == RefSource::Subject));
        assert!(has(RefKind::Email, "jo.rivera@brightwell.example"));
        assert!(has(RefKind::Domain, "brightwell.example"));
        assert!(has(RefKind::Domain, "example.co.uk"));
        assert!(has(RefKind::Pcn, "WK12345678"));
        assert!(has(RefKind::Pcn, "QX9912345"));
        assert!(has(RefKind::UkPlate, "AB12CDE"));
        // An internationalised domain in an email is stored as its A-label.
        let idn = values("mail jo@bücher.example", &[], LONDON, RefKind::Email);
        assert_eq!(idn, ["jo@xn--bcher-kva.example"]);
        assert_eq!(
            norm("example.net", RefKind::Domain).as_deref(),
            Some("example.net")
        );
        assert_eq!(
            norm("https://a.b.example.com/x", RefKind::Domain).as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn f5_custom_refs() {
        let custom = [
            CustomRef::new("booking", r"BK-\d{4,6}", Normalise::Upper).unwrap(),
            CustomRef::new("claim", r"claim ([a-z0-9]+)", Normalise::Upper).unwrap(),
        ];
        let refs = extract("", "BK-4821 and claim x77 ", &[], &custom, LONDON);
        let kinds: Vec<_> = refs
            .iter()
            .map(|r| (r.kind.as_str(), r.value.as_str()))
            .collect();
        assert!(kinds.contains(&("custom:booking".into(), "BK-4821")));
        assert!(kinds.contains(&("custom:claim".into(), "X77")));
        let c = candidates("BK-4821", &[], &custom, LONDON);
        assert_eq!(c[0].value, "BK-4821");
        assert_eq!(c.last().unwrap().value, "BK4821");
        // A pattern over the size limit is refused.
        assert!(CustomRef::new("big", r"\w{1000}\w{1000}", Normalise::None).is_err());
    }

    #[test]
    fn f5_caps() {
        let many: String = (0..80).map(|i| format!("Invoice 1{i:04} ")).collect();
        let refs = extract("", &many, &[], &[], LONDON);
        assert_eq!(refs.len(), MAX_PER_KIND);
        let lots: String = (0..300)
            .map(|i| format!("Invoice 1{i:04} order 2{i:04} £{i}.00 x{i}@a.example "))
            .collect();
        assert_eq!(extract("", &lots, &[], &[], LONDON).len(), MAX_PER_MESSAGE);
        let x = [CustomRef::new("x", r"X\d+", Normalise::None).unwrap()];
        let long = format!("X{} X{}", "2".repeat(64), "3".repeat(63));
        let kept = extract("", &long, &[], &x, LONDON);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].value.len(), MAX_VALUE_CHARS);
        // Duplicates collapse; the same value in subject and body keeps both sources.
        let r = extract(
            "Invoice 88213",
            "Invoice 88213, again Invoice 88213",
            &[],
            &[],
            LONDON,
        );
        assert_eq!(r.len(), 2);
        assert_eq!(
            RefSource::Attachment {
                attachment_id: "att_1".into(),
                page: 2
            }
            .to_string(),
            "att:att_1:2"
        );
    }
}
