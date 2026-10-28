//! Allow and block lists ([Parity additions § 3.1]): which entries match an address, and the effective
//! decision of an identity's and its tenant's entries.
//!
//! [Parity additions § 3.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/parity-additions.md

use crate::address::normalise_domain;

/// One stored entry that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    /// An identity-scope entry (else the tenant's).
    pub identity: bool,
    pub block: bool,
    /// `user@example.com` or `@example.com`.
    pub entry: String,
}

/// The effective decision and the entry that decided it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListDecision {
    pub block: bool,
    pub identity: bool,
    pub entry: String,
}

/// The entries that can match `address`: the address itself and `@` followed by its domain.
pub fn candidates(address: &str) -> [String; 2] {
    let domain = address.rsplit_once('@').map_or("", |(_, d)| d);
    [address.to_owned(), format!("@{domain}")]
}

/// The stored form of an entry: lower case with an A-label domain; `None` when it is neither an
/// address nor `@domain`.
pub fn normalise_entry(entry: &str) -> Option<String> {
    let e = entry.trim();
    let (local, domain) = e.rsplit_once('@')?;
    if local.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    let domain = normalise_domain(domain)?;
    Some(format!("{}@{domain}", local.to_lowercase()))
}

/// The decision over the rows that matched one address (both scopes).
pub fn decide(rows: &[ListRow]) -> Option<ListDecision> {
    let is_domain = |r: &ListRow| r.entry.starts_with('@');
    let identity_address = rows.iter().any(|r| r.identity && !is_domain(r));
    let identity_domain = rows.iter().any(|r| r.identity && is_domain(r));
    let left: Vec<&ListRow> = rows
        .iter()
        .filter(|r| r.identity || !(identity_address || (identity_domain && is_domain(r))))
        .collect();
    let pick = |block: bool| {
        left.iter()
            .filter(|r| r.block == block)
            .max_by_key(|r| r.identity)
            .map(|r| ListDecision {
                block,
                identity: r.identity,
                entry: r.entry.clone(),
            })
    };
    pick(true).or_else(|| pick(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(identity: bool, block: bool, entry: &str) -> ListRow {
        ListRow {
            identity,
            block,
            entry: entry.into(),
        }
    }

    #[test]
    fn precedence() {
        assert_eq!(decide(&[]), None);
        // Identity allow overrides a tenant block of the address and of its domain.
        let d = decide(&[
            row(false, true, "jo@x.net"),
            row(false, true, "@x.net"),
            row(true, false, "jo@x.net"),
        ])
        .unwrap();
        assert!(!d.block && d.identity);
        // An identity @domain allow never overrides a tenant block of the exact address.
        let d = decide(&[row(false, true, "jo@x.net"), row(true, false, "@x.net")]).unwrap();
        assert!(d.block && !d.identity);
        // It does override a tenant @domain block.
        let d = decide(&[row(false, true, "@x.net"), row(true, false, "@x.net")]).unwrap();
        assert!(!d.block);
        // Among what is left, block wins.
        let d = decide(&[row(false, false, "jo@x.net"), row(false, true, "@x.net")]).unwrap();
        assert!(d.block);
    }

    #[test]
    fn entries() {
        assert_eq!(
            normalise_entry("Jo@Bücher.example").as_deref(),
            Some("jo@xn--bcher-kva.example")
        );
        assert_eq!(
            normalise_entry("@Example.COM").as_deref(),
            Some("@example.com")
        );
        assert_eq!(normalise_entry("nope"), None);
        assert_eq!(
            candidates("jo@x.net"),
            ["jo@x.net".to_owned(), "@x.net".to_owned()]
        );
    }
}
