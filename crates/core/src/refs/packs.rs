//! The built-in normalisers of packs `core` and `uk_vehicle`. Patterns run over an ASCII-lower-cased
//! copy of the text (same byte offsets), so they are written in lower case; values are cut from the
//! original text.

use std::sync::OnceLock;

use chrono::NaiveDate;
use regex::Regex;

use super::phone::National;
use super::{Ctx, RefKind};
use crate::address::{normalise_domain, org_domain};

pub(super) struct Hit {
    pub kind: RefKind,
    /// Byte span of the whole match in the scanned text.
    pub start: usize,
    pub end: usize,
    pub value: String,
}

struct Patterns {
    amount: Regex,
    phone_intl: Regex,
    phone_trunk: Regex,
    phone_nanp: Regex,
    email: Regex,
    url: Regex,
    date_iso: Regex,
    date_numeric: Regex,
    date_text: Regex,
    invoice: Regex,
    order: Regex,
    plate: Regex,
    pcn: Regex,
}

const CURRENCY: &str = r"£|€|\$|gbp|eur|usd";
const NUMBER: &str = r"[0-9](?:[0-9.,]*[0-9])?";
/// `no.`, `number`, `nr.`, then `#` or `:`, each optional.
const SEPARATOR: &str = r"[ \t]*(?:(?:number|no|nr)\b\.?[ \t]*)?";
const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |p: &str| Regex::new(p).expect("built-in reference pattern");
        let months = MONTHS
            .iter()
            .copied()
            .chain(MONTHS.iter().map(|m| &m[..3]))
            .collect::<Vec<_>>()
            .join("|");
        Patterns {
            amount: re(&format!(
                r"(?:(?P<c1>{CURRENCY})\s?(?P<n1>{NUMBER}))|(?:(?P<n2>{NUMBER})\s?(?P<c2>{CURRENCY}))"
            )),
            phone_intl: re(r"(?:\+|\b00)[1-9][0-9 .()\-]{5,24}[0-9]"),
            phone_trunk: re(r"\(?0[0-9][0-9 .()\-]{5,18}[0-9]"),
            phone_nanp: re(r"(?:\b1[ .\-]?)?\(?[2-9][0-9]{2}\)?[ .\-]?[0-9]{3}[ .\-]?[0-9]{4}"),
            email: re(
                r"[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*@(?:\w(?:[\w-]*\w)?\.)+\w(?:[\w-]*\w)?",
            ),
            url: re(r#"https?://[^\s<>"'()\[\]]+"#),
            date_iso: re(r"\b([0-9]{4})-([0-9]{2})-([0-9]{2})\b"),
            date_numeric: re(r"\b([0-9]{1,2})([/.])([0-9]{1,2})([/.])([0-9]{4})\b"),
            date_text: re(&format!(r"\b([0-9]{{1,2}}) +({months}) +([0-9]{{4}})\b")),
            invoice: re(&format!(
                r"\binv-(?P<d>[0-9]+)\b|\b(?:invoice|inv|rechnung|facture|factura|fattura)\b{SEPARATOR}(?:[#:][ \t]*)?(?P<v>[a-z0-9][a-z0-9/-]{{2,24}})\b"
            )),
            order: re(&format!(
                r"\b(?:purchase order|order|ord|po|bestellung|commande|pedido)\b{SEPARATOR}(?:[#:-][ \t]*)?(?P<v>[a-z0-9][a-z0-9-]{{3,24}})\b"
            )),
            plate: re(
                r"\b(?:[a-z]{2}[0-9]{2}[ -]{0,3}[a-z]{3}|[a-z][0-9]{1,3}[ -]{0,3}[a-z]{3}|[a-z]{3}[ -]{0,3}[0-9]{1,3}[a-z])\b",
            ),
            pcn: re(
                r"\b(?P<a>[a-z]{2}[0-9]{8})\b|\b(?:pcn|penalty charge)\b[^\n]{0,20}?\b(?P<b>[a-z]*[0-9][a-z0-9]*)\b",
            ),
        }
    })
}

fn before(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

fn after(text: &str, i: usize) -> Option<char> {
    text[i..].chars().next()
}

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

pub(super) fn scan_core(text: &str, ctx: &Ctx<'_>, out: &mut Vec<Hit>) {
    let lower = text.to_ascii_lowercase();
    let p = patterns();
    let mut push = |kind: RefKind, start: usize, end: usize, value: String| {
        out.push(Hit {
            kind,
            start,
            end,
            value,
        });
    };

    // amount
    // A rejected match restarts one character later, so `20000 £5.00` still finds `£5.00`.
    let mut at = 0;
    while let Some(c) = p.amount.captures_at(&lower, at) {
        let (Some(whole), Some(code), Some(num)) = (
            c.get(0),
            c.name("c1").or_else(|| c.name("c2")),
            c.name("n1").or_else(|| c.name("n2")),
        ) else {
            break;
        };
        at = whole.start() + after(&lower, whole.start()).map_or(1, char::len_utf8);
        // A currency code or a number must not continue a word or another number.
        let starts_word = whole
            .as_str()
            .starts_with(|ch: char| ch.is_ascii_alphanumeric());
        let left_ok = !starts_word
            || before(&lower, whole.start())
                .is_none_or(|ch| !(ch.is_alphanumeric() || matches!(ch, '_' | '.' | ',')));
        let right_ok = after(&lower, whole.end()).is_none_or(|ch| !ch.is_alphanumeric());
        if !(left_ok && right_ok) {
            continue;
        }
        at = whole.end();
        let iso = match code.as_str() {
            "£" | "gbp" => "GBP",
            "€" | "eur" => "EUR",
            _ => "USD",
        };
        if let Some(v) = amount(num.as_str(), iso) {
            push(RefKind::Amount, whole.start(), whole.end(), v);
        }
    }

    // phone: international first; national matches that overlap one are skipped.
    let mut intl: Vec<(usize, usize)> = Vec::new();
    for m in p.phone_intl.find_iter(&lower) {
        let bounded = before(&lower, m.start()).is_none_or(|c| !c.is_alphanumeric())
            && after(&lower, m.end()).is_none_or(|c| !c.is_ascii_digit());
        let rest = m.as_str().strip_prefix('+').unwrap_or(&m.as_str()[2..]);
        let digits: String = rest
            .replace("(0)", "")
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        if bounded && (7..=15).contains(&digits.len()) && balanced(m.as_str()) {
            intl.push((m.start(), m.end()));
            push(RefKind::Phone, m.start(), m.end(), format!("+{digits}"));
        }
    }
    let national = match ctx.country.national {
        National::Trunk { .. } => &p.phone_trunk,
        National::Nanp => &p.phone_nanp,
    };
    for m in national.find_iter(&lower) {
        if intl.iter().any(|&(s, e)| m.start() < e && s < m.end()) {
            continue;
        }
        let bounded = before(&lower, m.start()).is_none_or(|c| !c.is_alphanumeric() && c != '+')
            && after(&lower, m.end()).is_none_or(|c| !c.is_ascii_digit());
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        let e164 = match ctx.country.national {
            National::Trunk {
                min_digits,
                max_digits,
            } => (min_digits..=max_digits)
                .contains(&digits.len())
                .then(|| format!("+{}{}", ctx.country.calling_code, &digits[1..])),
            National::Nanp => match digits.len() {
                10 => Some(format!("+1{digits}")),
                11 if digits.starts_with('1') => Some(format!("+{digits}")),
                _ => None,
            },
        };
        if let (true, true, Some(v)) = (bounded, balanced(m.as_str()), e164) {
            push(RefKind::Phone, m.start(), m.end(), v);
        }
    }

    // email, and its domain
    for m in p.email.find_iter(&lower) {
        let Some((local, domain)) = text[m.start()..m.end()].rsplit_once('@') else {
            continue;
        };
        if let Some(d) = normalise_domain(domain).filter(|d| has_alpha_tld(d)) {
            let local = local.to_ascii_lowercase();
            push(RefKind::Email, m.start(), m.end(), format!("{local}@{d}"));
            let at = m.start() + local.len() + 1;
            push(RefKind::Domain, at, m.end(), org_domain(&d).to_owned());
        }
    }

    // domain of http(s) URLs
    for m in p.url.find_iter(&lower) {
        if let Some(d) = url_domain(&text[m.start()..m.end()]) {
            push(RefKind::Domain, m.start(), m.end(), d);
        }
    }

    // date
    for c in p.date_iso.captures_iter(&lower) {
        let (Some(w), Some(y), Some(mo), Some(d)) = (c.get(0), c.get(1), c.get(2), c.get(3)) else {
            continue;
        };
        if let Some(v) = date(y.as_str(), mo.as_str(), d.as_str()) {
            push(RefKind::Date, w.start(), w.end(), v);
        }
    }
    for c in p.date_numeric.captures_iter(&lower) {
        let (Some(w), Some(a), Some(s1), Some(b), Some(s2), Some(y)) =
            (c.get(0), c.get(1), c.get(2), c.get(3), c.get(4), c.get(5))
        else {
            continue;
        };
        if s1.as_str() != s2.as_str() {
            continue;
        }
        let first_is_day = ctx.day_first || a.as_str().parse::<u32>().is_ok_and(|n| n > 12);
        let (d, mo) = if first_is_day { (a, b) } else { (b, a) };
        if let Some(v) = date(y.as_str(), mo.as_str(), d.as_str()) {
            push(RefKind::Date, w.start(), w.end(), v);
        }
    }
    for c in p.date_text.captures_iter(&lower) {
        let (Some(w), Some(d), Some(mo), Some(y)) = (c.get(0), c.get(1), c.get(2), c.get(3)) else {
            continue;
        };
        let month = MONTHS
            .iter()
            .position(|m| m.starts_with(mo.as_str()))
            .map(|i| (i + 1).to_string());
        if let Some(v) = month.and_then(|mo| date(y.as_str(), &mo, d.as_str())) {
            push(RefKind::Date, w.start(), w.end(), v);
        }
    }

    // invoice: `INV-` plus digits gives the digits only.
    for c in p.invoice.captures_iter(&lower) {
        let Some(w) = c.get(0) else { continue };
        let value = if let Some(d) = c.name("d") {
            d.as_str().to_owned()
        } else if let Some(v) = c.name("v").filter(|v| has_digit(v.as_str())) {
            let v = text[v.start()..v.end()].to_ascii_uppercase();
            match v.strip_prefix("INV-") {
                Some(d) if d.bytes().all(|b| b.is_ascii_digit()) => d.to_owned(),
                _ => v,
            }
        } else {
            continue;
        };
        push(RefKind::Invoice, w.start(), w.end(), value);
    }

    // order
    for c in p.order.captures_iter(&lower) {
        if let (Some(w), Some(v)) = (c.get(0), c.name("v").filter(|v| has_digit(v.as_str()))) {
            let v = text[v.start()..v.end()].to_ascii_uppercase();
            push(RefKind::Order, w.start(), w.end(), v);
        }
    }
}

pub(super) fn scan_uk_vehicle(text: &str, out: &mut Vec<Hit>) {
    let lower = text.to_ascii_lowercase();
    let p = patterns();
    for m in p.plate.find_iter(&lower) {
        out.push(Hit {
            kind: RefKind::UkPlate,
            start: m.start(),
            end: m.end(),
            value: plate(&text[m.start()..m.end()]),
        });
    }
    for c in p.pcn.captures_iter(&lower) {
        let Some(w) = c.get(0) else { continue };
        let Some(v) = c
            .name("a")
            .or_else(|| c.name("b").filter(|b| (8..=12).contains(&b.len())))
        else {
            continue;
        };
        out.push(Hit {
            kind: RefKind::Pcn,
            start: w.start(),
            end: w.end(),
            value: text[v.start()..v.end()].to_ascii_uppercase(),
        });
    }
}

/// Upper case, spaces and dashes removed.
fn plate(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-'))
        .collect::<String>()
        .to_ascii_uppercase()
}

/// `{ISO}:{amount with two decimals}`. A comma is the decimal separator only for euro amounts of the
/// form `\d+,\d{2}`; otherwise commas group thousands (`\d{1,3}(,\d{3})+`) and `.` is the decimal point.
fn amount(num: &str, iso: &str) -> Option<String> {
    let euro_comma = iso == "EUR"
        && num.split_once(',').is_some_and(|(i, f)| {
            !i.is_empty()
                && f.len() == 2
                && (i.bytes().chain(f.bytes())).all(|b| b.is_ascii_digit())
        });
    let (int, frac) = if euro_comma {
        num.split_once(',')?
    } else {
        num.split_once('.').unwrap_or((num, ""))
    };
    if num.ends_with('.') || frac.len() > 2 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut groups = int.split(',');
    let first = groups.next()?;
    let rest: Vec<&str> = groups.collect();
    let grouped_ok = rest.is_empty() || (first.len() <= 3 && rest.iter().all(|g| g.len() == 3));
    let digits: String = int.chars().filter(|&c| c != ',').collect();
    if first.is_empty() || !grouped_ok || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.len() > 15 {
        return None;
    }
    let int = if digits.is_empty() { "0" } else { digits };
    Some(format!("{iso}:{int}.{frac:0<2}"))
}

fn date(y: &str, m: &str, d: &str) -> Option<String> {
    let (y, m, d) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    NaiveDate::from_ymd_opt(y, m, d)?;
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Parentheses in a phone match pair up (a match never starts inside someone else's bracket).
fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if !(0..=1).contains(&depth) {
            return false;
        }
    }
    depth == 0
}

fn has_alpha_tld(domain: &str) -> bool {
    domain
        .rsplit('.')
        .next()
        .is_some_and(|t| t.bytes().any(|b| b.is_ascii_alphabetic()))
}

/// The registrable domain of an `http(s)` URL's host (no userinfo, no port).
fn url_domain(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next()?;
    let d = normalise_domain(host).filter(|d| has_alpha_tld(d))?;
    Some(org_domain(&d).to_owned())
}

/// A bare domain name, URL or neither: its registrable domain. Used for `ref:` values only.
pub(super) fn bare_domain(raw: &str) -> Option<String> {
    if raw.contains("://") {
        return None; // URLs are matched by the scanner
    }
    if raw.contains(['@', ' ', '/']) || !raw.contains('.') {
        return None;
    }
    let d = normalise_domain(raw).filter(|d| has_alpha_tld(d))?;
    Some(org_domain(&d).to_owned())
}
