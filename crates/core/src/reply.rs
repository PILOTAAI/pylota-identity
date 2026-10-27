//! Who a reply goes to ([Outbound › Recipients], D3). For unknown senders a reply goes to `From`; a
//! `Reply-To` is used only when it is safe.
//!
//! [Outbound › Recipients]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use crate::address::{domain_of, org_domain};

/// What decides the reply target of an inbound message M.
pub struct ReplyInputs<'a> {
    /// M's normalised `From` address.
    pub from: &'a str,
    /// M's normalised `Reply-To` addresses, header order.
    pub reply_to: &'a [String],
    /// Trust signal `known_sender` of M.
    pub known_sender: bool,
}

/// Whether the identity has written to an address (`contacts.outbound_count > 0`).
pub trait Contacts {
    fn written_to(&self, address: &str) -> bool;
}

/// The first `Reply-To` address when the sender is known, or it shares the organisational domain of
/// `From`, or the identity has written to it; otherwise `From`.
pub fn reply_target<'a>(inp: &ReplyInputs<'a>, contacts: &impl Contacts) -> &'a str {
    let Some(first) = inp.reply_to.first() else {
        return inp.from;
    };
    if inp.known_sender || same_org(first, inp.from) || contacts.written_to(first) {
        first
    } else {
        inp.from
    }
}

/// Trust flag `reply_to_mismatch`: a `Reply-To` whose organisational domain differs from `From`'s.
pub fn reply_to_mismatch(from: &str, reply_to: &[String]) -> bool {
    reply_to.iter().any(|r| !same_org(r, from))
}

fn same_org(a: &str, b: &str) -> bool {
    org_domain(domain_of(a)) == org_domain(domain_of(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Written(&'static [&'static str]);
    impl Contacts for Written {
        fn written_to(&self, address: &str) -> bool {
            self.0.contains(&address)
        }
    }

    /// Covers: D3
    #[test]
    fn d3_reply_target() {
        let none = Written(&[]);
        let inp = |reply_to: &'static [&'static str], known| {
            (
                reply_to.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
                known,
            )
        };
        let check = |(rt, known): (Vec<String>, bool), contacts: &Written| {
            let i = ReplyInputs {
                from: "ana@brightwell.example.com",
                reply_to: &rt,
                known_sender: known,
            };
            reply_target(&i, contacts).to_owned()
        };
        // No Reply-To.
        assert_eq!(check(inp(&[], false), &none), "ana@brightwell.example.com");
        // Unknown sender, foreign Reply-To: From.
        assert_eq!(
            check(inp(&["pay@evil.example.net"], false), &none),
            "ana@brightwell.example.com"
        );
        // Known sender: the first Reply-To.
        assert_eq!(
            check(inp(&["pay@evil.example.net", "x@y.example"], true), &none),
            "pay@evil.example.net"
        );
        // Same organisational domain.
        assert_eq!(
            check(inp(&["desk@help.example.com"], false), &none),
            "desk@help.example.com"
        );
        // Written to before.
        assert_eq!(
            check(
                inp(&["pay@evil.example.net"], false),
                &Written(&["pay@evil.example.net"])
            ),
            "pay@evil.example.net"
        );
        assert!(reply_to_mismatch(
            "ana@brightwell.example.com",
            &["pay@evil.example.net".into()]
        ));
        assert!(!reply_to_mismatch(
            "ana@brightwell.example.com",
            &["desk@help.example.com".into()]
        ));
        assert!(!reply_to_mismatch("ana@brightwell.example.com", &[]));
    }
}
