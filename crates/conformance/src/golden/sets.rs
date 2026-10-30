//! The labelled sets (Search § 13.2–13.3, Triage § 13): `queries.toml`, `questions.toml`,
//! `triage.toml`, and the hand-written messages of `hard_cases/`. Relevance is written as selectors
//! over the ground truth, so a label names what makes a message relevant ("every message stating
//! `BK-2291`") rather than a list that would drift from the generator.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::generator::{Message, TriageTruth, Truth};

/// What a relevance label or a gold support entry matches; every field given must hold.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub key: Option<String>,
    /// A reference the message states, compared without spaces, hyphens and case.
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub domain: Option<String>,
    pub category: Option<String>,
    pub topic: Option<String>,
    pub lang: Option<String>,
    pub identity: Option<String>,
    pub triage_category: Option<String>,
    pub max_age_days: Option<u32>,
    pub min_age_days: Option<u32>,
    pub not_category: Option<String>,
    pub not_topic: Option<String>,
    /// `fact = "value"` pairs the truth must hold.
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

impl Selector {
    pub fn matches(&self, t: &Truth) -> bool {
        let eq = |want: &Option<String>, got: &str| want.as_deref().is_none_or(|w| w == got);
        eq(&self.key, &t.key)
            && self
                .reference
                .as_deref()
                .is_none_or(|r| t.refs.iter().any(|x| squash(x) == squash(r)))
            && eq(&self.domain, &t.domain)
            && eq(&self.category, &t.category)
            && eq(&self.topic, &t.topic)
            && eq(&self.lang, &t.lang)
            && eq(&self.identity, &t.identity)
            && eq(&self.triage_category, &t.triage.category)
            && self.max_age_days.is_none_or(|d| t.age_days <= d)
            && self.min_age_days.is_none_or(|d| t.age_days >= d)
            && self.not_category.as_deref().is_none_or(|c| c != t.category)
            && self.not_topic.as_deref().is_none_or(|c| c != t.topic)
            && self.facts.iter().all(|(k, v)| t.facts.get(k) == Some(v))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graded {
    pub grade: u8,
    #[serde(flatten)]
    pub selector: Selector,
}

/// One labelled search query, run at tenant scope.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub id: String,
    /// `exact_reference`, `sender`, `operators`, `paraphrase`, `multilingual`, `typos`,
    /// `relative_dates`, `negation_or`.
    pub kind: String,
    pub q: String,
    pub relevant: Vec<Graded>,
}

impl Query {
    /// Each relevant message's key with its highest grade (grade 0 entries are not relevant).
    pub fn grades(&self, truths: &[&Truth]) -> BTreeMap<String, u8> {
        let mut out = BTreeMap::new();
        for t in truths {
            for g in &self.relevant {
                if g.grade > 0 && g.selector.matches(t) {
                    let e = out.entry(t.key.clone()).or_insert(0);
                    *e = (*e).max(g.grade);
                }
            }
        }
        out
    }
}

/// One agentic question with its gold facts and support.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub q: String,
    pub answerable: bool,
    /// Key facts every correct answer contains: literal, or `{name}` for a fact of the support.
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub support: Vec<Selector>,
    /// The mail the question touches carries a steering attempt.
    #[serde(default)]
    pub steering: bool,
    /// Text a steered answer would contain; it must never appear.
    #[serde(default)]
    pub forbidden: Vec<String>,
}

impl Question {
    /// The key facts, with each `{name}` taken from the first supporting message that states the fact
    /// `name` (the generator draws amounts and dates, so the file cannot spell them out).
    pub fn resolved_facts(&self, truths: &[&Truth]) -> Result<Vec<String>, String> {
        let support: Vec<&&Truth> = truths
            .iter()
            .filter(|t| self.support.iter().any(|s| s.matches(t)))
            .collect();
        self.facts
            .iter()
            .map(
                |f| match f.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
                    Some(name) => support
                        .iter()
                        .find_map(|t| t.facts.get(name).cloned())
                        .ok_or_else(|| format!("{}: no supporting message states {name}", self.id)),
                    None => Ok(f.clone()),
                },
            )
            .collect()
    }

    pub fn support_keys(&self, truths: &[&Truth]) -> Vec<String> {
        truths
            .iter()
            .filter(|t| self.support.iter().any(|s| s.matches(t)))
            .map(|t| t.key.clone())
            .collect()
    }
}

/// `triage.toml`: every `stride`th generated message, plus the hand-written cases.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageSet {
    pub stride: usize,
    #[serde(default, rename = "case")]
    pub cases: Vec<TriageCase>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageCase {
    pub key: String,
    pub category: String,
    pub needs_reply: bool,
    pub urgency: u8,
    #[serde(default)]
    pub risk_flags: Vec<String>,
}

/// A hand-written message: `hard_cases/{key}.eml` (with `{to}` where the recipient goes) and its
/// truth in `hard_cases/cases.toml`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardCase {
    pub key: String,
    pub identity: String,
    pub topic: String,
    #[serde(default)]
    pub refs: Vec<String>,
    pub from: String,
    #[serde(default = "en")]
    pub lang: String,
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}

fn en() -> String {
    "en".into()
}

#[derive(Deserialize)]
struct QueriesFile {
    query: Vec<Query>,
}

#[derive(Deserialize)]
struct QuestionsFile {
    question: Vec<Question>,
}

#[derive(Deserialize)]
struct CasesFile {
    case: Vec<HardCase>,
}

/// A hand-written message ready to inject: its truth and its raw text with `{to}` unfilled.
#[derive(Clone, Debug)]
pub struct Hard {
    pub truth: Truth,
    pub raw: String,
}

/// Everything in `golden/`.
pub struct Golden {
    pub config: super::generator::Config,
    pub generated: Vec<Message>,
    pub hard: Vec<Hard>,
    pub queries: Vec<Query>,
    pub questions: Vec<Question>,
    pub triage: TriageSet,
}

fn read(dir: &Path, name: &str) -> Result<String, String> {
    std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))
}

impl Golden {
    /// Loads `dir` (normally [`super::dir`]) and generates the mailbox as of `now_ms`.
    pub fn load(dir: &Path, now_ms: i64) -> Result<Golden, String> {
        let config: super::generator::Config = toml::from_str(&read(dir, "generator.toml")?)
            .map_err(|e| format!("generator.toml: {e}"))?;
        let generated = super::generator::generate(&config, now_ms);
        let queries = toml::from_str::<QueriesFile>(&read(dir, "queries.toml")?)
            .map_err(|e| format!("queries.toml: {e}"))?
            .query;
        let questions = toml::from_str::<QuestionsFile>(&read(dir, "questions.toml")?)
            .map_err(|e| format!("questions.toml: {e}"))?
            .question;
        let triage: TriageSet =
            toml::from_str(&read(dir, "triage.toml")?).map_err(|e| format!("triage.toml: {e}"))?;
        let cases = toml::from_str::<CasesFile>(&read(dir, "hard_cases/cases.toml")?)
            .map_err(|e| format!("hard_cases/cases.toml: {e}"))?
            .case;
        let labels: BTreeMap<&str, &TriageCase> =
            triage.cases.iter().map(|c| (c.key.as_str(), c)).collect();
        let mut hard = Vec::new();
        for c in cases {
            let raw = read(dir, &format!("hard_cases/{}.eml", c.key))?;
            let label = labels.get(c.key.as_str());
            let t = Truth {
                key: c.key.clone(),
                identity: c.identity,
                category: "hard".into(),
                topic: c.topic,
                refs: c.refs,
                domain: c.from.rsplit('@').next().unwrap_or("").to_owned(),
                from: c.from,
                lang: c.lang,
                age_days: 1,
                thread: c.key.clone(),
                facts: c.facts,
                triage: label.map_or(
                    TriageTruth {
                        category: "other".into(),
                        needs_reply: false,
                        urgency: 0,
                        risk_flags: Vec::new(),
                    },
                    |l| TriageTruth {
                        category: l.category.clone(),
                        needs_reply: l.needs_reply,
                        urgency: l.urgency,
                        risk_flags: l.risk_flags.clone(),
                    },
                ),
            };
            hard.push(Hard { truth: t, raw });
        }
        Ok(Golden {
            config,
            generated,
            hard,
            queries,
            questions,
            triage,
        })
    }

    /// Every message's truth: generated first, then the hand-written ones.
    pub fn truths(&self) -> Vec<&Truth> {
        self.generated
            .iter()
            .map(|m| &m.truth)
            .chain(self.hard.iter().map(|h| &h.truth))
            .collect()
    }

    /// The labelled triage set: `(truth key, labels)`, every `stride`th generated message and every
    /// hand-written case.
    pub fn triage_items(&self) -> Vec<(String, TriageTruth)> {
        let mut out: Vec<(String, TriageTruth)> = self
            .generated
            .iter()
            .step_by(self.triage.stride.max(1))
            .map(|m| (m.truth.key.clone(), m.truth.triage.clone()))
            .collect();
        out.extend(
            self.hard
                .iter()
                .filter(|h| self.triage.cases.iter().any(|c| c.key == h.truth.key))
                .map(|h| (h.truth.key.clone(), h.truth.triage.clone())),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_000_000_000;

    fn golden() -> Golden {
        Golden::load(&crate::golden::dir(), NOW).unwrap()
    }

    /// The sets meet the sizes of Search § 13.2–13.3 and Triage § 13, and every label resolves.
    #[test]
    fn sets_are_complete() {
        let g = golden();
        let truths = g.truths();
        assert!(g.queries.len() >= 200, "{} queries", g.queries.len());
        let want = [
            ("exact_reference", 40),
            ("sender", 20),
            ("operators", 30),
            ("paraphrase", 50),
            ("multilingual", 15),
            ("typos", 20),
            ("relative_dates", 15),
            ("negation_or", 10),
        ];
        for (kind, n) in want {
            let got = g.queries.iter().filter(|q| q.kind == kind).count();
            assert!(got >= n, "{kind}: {got} of {n}");
        }
        for q in &g.queries {
            assert!(
                !q.grades(&truths).is_empty(),
                "query {} has no relevant message",
                q.id
            );
        }
        let ids: std::collections::BTreeSet<_> = g.queries.iter().map(|q| &q.id).collect();
        assert_eq!(ids.len(), g.queries.len(), "query ids are unique");

        assert!(g.questions.len() >= 50);
        assert!(g.questions.iter().filter(|q| !q.answerable).count() >= 10);
        assert!(g.questions.iter().filter(|q| q.steering).count() >= 5);
        for q in &g.questions {
            if q.answerable {
                assert!(!q.facts.is_empty(), "{}", q.id);
                assert!(
                    !q.support_keys(&truths).is_empty(),
                    "{} has no support",
                    q.id
                );
                q.resolved_facts(&truths).unwrap();
            }
        }

        let items = g.triage_items();
        assert!(items.len() >= 750, "{} triage items", items.len());
        assert!(g.triage.cases.len() >= 150);
        for c in &g.triage.cases {
            assert!(
                g.hard.iter().any(|h| h.truth.key == c.key),
                "triage case {} has no message",
                c.key
            );
            assert!(
                pylota_identity_core::triage_rules::BUILT_IN_CATEGORIES
                    .contains(&c.category.as_str()),
                "{}",
                c.key
            );
        }
        for h in &g.hard {
            assert!(h.raw.contains("{to}"), "{} has no {{to}}", h.truth.key);
        }
    }
}
