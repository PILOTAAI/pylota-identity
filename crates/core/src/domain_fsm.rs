//! The domain state machine ([Identities and domains › Domain health], FR-DOM-4, FR-DOM-5, ADR 0005):
//! per-resolver outcomes, two-resolver agreement and the two-cycle rule (H7), the both-resolvers-failing
//! escalation to `dnssec_bogus` or `dns_unresolvable` (C8), the transition table, the platform
//! domain's rules (H9) and reminders.
//!
//! `DomainMonitor` does the I/O (DoH queries, provider calls, D1 and the outbox) and calls [`step`]
//! with what it saw; time is the `now_ms` argument.
//!
//! [Identities and domains › Domain health]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md

use serde::{Deserialize, Serialize};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// Both resolvers failing on every cycle for this long triggers the `cd=1` re-query (C8).
pub const BOTH_ERROR_ESCALATION_MS: i64 = 6 * HOUR;
/// A never-verified domain is removed after this long ([H14]); `failing` suspends after it too.
///
/// [H14]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md
pub const FOURTEEN_DAYS_MS: i64 = 14 * DAY;
/// The next check after a full check, after a check that started a transition, and after a cycle
/// with no agreed outcome ([Schedule]).
///
/// [Schedule]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
pub const CHECK_EVERY_MS: i64 = 15 * MINUTE;
pub const CONFIRM_AFTER_MS: i64 = 5 * MINUTE;
pub const RETRY_AFTER_MS: i64 = 2 * MINUTE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Verifying,
    Healthy,
    Degraded,
    Failing,
    Suspended,
    Removing,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipReason {
    NameserversChanged,
    OwnershipRecordMissing,
    RegistrationChanged,
}

impl OwnershipReason {
    pub fn code(self) -> &'static str {
        match self {
            OwnershipReason::NameserversChanged => "nameservers_changed",
            OwnershipReason::OwnershipRecordMissing => "ownership_record_missing",
            OwnershipReason::RegistrationChanged => "registration_changed",
        }
    }
}

/// One resolver's outcome of a check ([Outcome per resolver and agreement] step 1).
///
/// [Outcome per resolver and agreement]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Degraded,
    Fail,
    /// The first ownership-level issue.
    OwnershipChanged(OwnershipReason),
    /// A query failed: timeout, HTTP error, `SERVFAIL`, `REFUSED`.
    Error,
}

/// The issue a `cd=1` re-query decides (C8), both fail-level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsIssue {
    DnssecBogus,
    DnsUnresolvable,
}

impl DnsIssue {
    pub fn code(self) -> &'static str {
        match self {
            DnsIssue::DnssecBogus => "dnssec_bogus",
            DnsIssue::DnsUnresolvable => "dns_unresolvable",
        }
    }
}

/// A failing name that answers with DNSSEC validation off is `dnssec_bogus`; one that still fails is
/// `dns_unresolvable`.
pub fn cd1_issue(answers_with_cd: bool) -> DnsIssue {
    if answers_with_cd {
        DnsIssue::DnssecBogus
    } else {
        DnsIssue::DnsUnresolvable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reminder {
    /// 24 hours, 72 hours and 7 days in the state; reset on every state change.
    Hours24,
    Hours72,
    Days7,
    /// A never-verified domain, two days before its unverified expiry.
    Day12,
    /// A `nameservers` zone still pending, before Cloudflare deletes it at day 28 (N23).
    Day21,
}

impl Reminder {
    fn in_state(self) -> bool {
        matches!(
            self,
            Reminder::Hours24 | Reminder::Hours72 | Reminder::Days7
        )
    }
}

/// What the monitor stores for its domain: the row's facts and `meta`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    pub state: State,
    /// `kind = 'platform'`: never suspended, ownership counts as `fail` (H9).
    pub platform: bool,
    /// Method `nameservers` or `delegated_subdomain`: the zone is created and must become active.
    pub creates_zone: bool,
    pub created_at_ms: i64,
    /// When the created zone was first seen active.
    pub zone_active_at_ms: Option<i64>,
    pub ownership_verified_at_ms: Option<i64>,
    pub state_changed_at_ms: i64,
    pub failing_since_ms: Option<i64>,
    /// `meta.candidate` and `meta.candidate_count`.
    pub candidate: Option<Outcome>,
    pub candidate_count: u8,
    /// `meta.both_error_since`.
    pub both_error_since_ms: Option<i64>,
    /// Consecutive cycles the new ownership TXT was seen on both resolvers while `suspended`.
    pub ownership_seen: u8,
    /// `meta.reminders_sent_json`.
    pub reminders_sent: Vec<Reminder>,
}

impl Domain {
    /// A new `pending` row.
    pub fn new(platform: bool, creates_zone: bool, now_ms: i64) -> Domain {
        Domain {
            state: State::Pending,
            platform,
            creates_zone,
            created_at_ms: now_ms,
            zone_active_at_ms: None,
            ownership_verified_at_ms: None,
            state_changed_at_ms: now_ms,
            failing_since_ms: None,
            candidate: None,
            candidate_count: 0,
            both_error_since_ms: None,
            ownership_seen: 0,
            reminders_sent: Vec::new(),
        }
    }

    /// Where the 14-day unverified expiry counts from: creation, or the zone's activation for
    /// `nameservers` and `delegated_subdomain`.
    fn expiry_anchor(&self) -> Option<i64> {
        if self.creates_zone {
            self.zone_active_at_ms
        } else {
            Some(self.created_at_ms)
        }
    }
}

/// Whether this cycle's names must be re-queried with `cd=1` before [`step`]: both resolvers
/// failed, and have on every cycle for 6 hours.
pub fn cd1_due(d: &Domain, resolvers: [Outcome; 2], now_ms: i64) -> bool {
    resolvers == [Outcome::Error, Outcome::Error]
        && d.both_error_since_ms
            .is_some_and(|s| now_ms - s >= BOTH_ERROR_ESCALATION_MS)
}

/// The state of a created zone, read while the domain is `pending`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneStatus {
    Pending,
    /// Cloudflare deleted it (N23).
    Gone,
    /// Active; `onboarded` once every onboarding step 2–8 has succeeded.
    Active {
        onboarded: bool,
    },
}

/// What one check saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub resolvers: [Outcome; 2],
    /// The `cd=1` decision, when [`cd1_due`] said to re-query: any name `dnssec_bogus` gives that.
    pub cd1: Option<DnsIssue>,
    /// For a pending domain that creates its zone.
    pub zone: Option<ZoneStatus>,
    /// A `nameservers` domain whose parent still publishes DS.
    pub ds_record_present: bool,
    /// While `suspended`: the new ownership TXT is on both resolvers.
    pub new_ownership_seen: bool,
}

impl Check {
    pub fn of(resolvers: [Outcome; 2]) -> Check {
        Check {
            resolvers,
            cd1: None,
            zone: None,
            ds_record_present: false,
            new_ownership_seen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveReason {
    Requested,
    Evicted,
    UnverifiedExpired,
}

impl RemoveReason {
    pub fn code(self) -> &'static str {
        match self {
            RemoveReason::Requested => "requested",
            RemoveReason::Evicted => "evicted",
            RemoveReason::UnverifiedExpired => "unverified_expired",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Init {
        /// The zone is active, or the kind is `external`.
        zone_active: bool,
    },
    Check(Check),
    /// A proved claim of another tenant ([Adding a domain]).
    ///
    /// [Adding a domain]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
    Evict,
    /// `DELETE …/domains/{id}`.
    Delete {
        /// An `active` or `retiring` address is on the domain.
        live_addresses: bool,
        /// The domain's `domain_remove` job is `failed`.
        job_failed: bool,
    },
    /// `POST …/reprove` (for the platform domain, with a platform key: the caller checks the key).
    Reprove,
    JobCompleted(RemoveReason),
    /// 10 attempts on one step.
    JobFailed,
    /// The daily restart of a failed removal.
    DailyRestart {
        job_failed: bool,
    },
}

/// Webhook events the step emits (through the monitor's outbox).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Verified,
    Degraded,
    Failing,
    Recovered { from: State },
    Suspended { reason: &'static str },
    Removed { reason: &'static str },
    Reminder(Reminder),
}

/// Effects for the monitor to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CheckNow,
    /// Activate the domain's `pending` addresses that are routed.
    ActivateAddresses,
    NewOwnershipToken,
    /// Re-read `records_json` from the providers.
    UpdateRecords,
    /// Re-record `expected_ns_json` and `rdap_fingerprint`.
    RerecordBaselines,
    /// Store the `pending` addresses' `(zone_id, routing_rule_id)` pairs in the job's `params_json`.
    StorePendingRules,
    DeletePendingAddresses,
    CreateRemoveJob(RemoveReason),
    /// Resume the removal at the first step not `done` (H12).
    RestartRemoveJob,
    /// `audit_log` `domain.evict`.
    AuditEvict,
    /// The platform domain's reprove: re-read the zone and `expected_ns_json`.
    RereadZone,
    /// Re-query this cycle's failing names with `cd=1` and step again.
    RequeryCd1,
    /// An operator alert by name.
    Alert(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `403 scope_denied`: the platform domain.
    ScopeDenied,
    /// `409 domain_in_use`: an `active` or `retiring` address.
    DomainInUse,
    /// The input does not apply in this state (a verified holder is never evicted, for example).
    NotApplicable,
}

/// The result of one [`step`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub from: State,
    pub to: State,
    /// `state_reason` when the step sets one; otherwise the monitor uses the first issue code.
    pub reason: Option<&'static str>,
    /// The cycle's agreed outcome (after the `cd=1` and platform rules), if any.
    pub agreed: Option<Outcome>,
    /// Issue codes the step itself adds (`dnssec_bogus`, `dns_unresolvable`, `ds_record_present`).
    pub issues: Vec<&'static str>,
    pub events: Vec<Event>,
    pub actions: Vec<Action>,
    pub refused: Option<Refusal>,
    /// Milliseconds until the next check, for a check.
    pub next_check_ms: Option<i64>,
}

impl Step {
    fn new(from: State) -> Step {
        Step {
            from,
            to: from,
            reason: None,
            agreed: None,
            issues: Vec::new(),
            events: Vec::new(),
            actions: Vec::new(),
            refused: None,
            next_check_ms: None,
        }
    }

    fn refuse(mut self, r: Refusal) -> Step {
        self.refused = Some(r);
        self
    }
}

/// Applies `input` to `d` at `now_ms`, updating `d` in place.
pub fn step(d: &mut Domain, input: Input, now_ms: i64) -> Step {
    let mut s = Step::new(d.state);
    match input {
        Input::Init { zone_active } => {
            if d.state == State::Pending && zone_active {
                if d.creates_zone && d.zone_active_at_ms.is_none() {
                    d.zone_active_at_ms = Some(now_ms);
                }
                enter(d, &mut s, State::Verifying, now_ms);
                s.actions.push(Action::CheckNow);
            } else if d.state == State::Pending {
                s.next_check_ms = Some(CHECK_EVERY_MS);
            } else {
                s = s.refuse(Refusal::NotApplicable);
            }
        }
        Input::Check(c) => check(d, &c, now_ms, &mut s),
        Input::Evict => {
            if matches!(d.state, State::Pending | State::Verifying)
                && d.ownership_verified_at_ms.is_none()
            {
                remove(d, &mut s, RemoveReason::Evicted, now_ms);
                s.actions.push(Action::AuditEvict);
            } else {
                s = s.refuse(Refusal::NotApplicable);
            }
        }
        Input::Delete {
            live_addresses,
            job_failed,
        } => match d.state {
            State::Removing if job_failed => s.actions.push(Action::RestartRemoveJob),
            State::Removing | State::Removed => s = s.refuse(Refusal::NotApplicable),
            _ if d.platform => s = s.refuse(Refusal::ScopeDenied),
            _ if live_addresses => s = s.refuse(Refusal::DomainInUse),
            _ => {
                s.actions.push(Action::StorePendingRules);
                remove(d, &mut s, RemoveReason::Requested, now_ms);
            }
        },
        Input::Reprove => match d.state {
            State::Suspended if !d.platform => {
                d.ownership_seen = 0;
                s.actions.extend([
                    Action::NewOwnershipToken,
                    Action::UpdateRecords,
                    Action::CheckNow,
                ]);
            }
            State::Failing if d.platform => {
                s.actions.extend([Action::RereadZone, Action::CheckNow]);
            }
            _ => s = s.refuse(Refusal::NotApplicable),
        },
        Input::JobCompleted(reason) if d.state == State::Removing => {
            enter(d, &mut s, State::Removed, now_ms);
            s.events.push(Event::Removed {
                reason: reason.code(),
            });
        }
        Input::JobFailed if d.state == State::Removing => {
            s.actions.push(Action::Alert("domain_remove_failed"));
        }
        Input::DailyRestart { job_failed: true } if d.state == State::Removing => {
            s.actions.push(Action::RestartRemoveJob);
        }
        Input::JobCompleted(_) | Input::JobFailed | Input::DailyRestart { .. } => {
            s = s.refuse(Refusal::NotApplicable);
        }
    }
    s
}

/// Moves to `to`: the state's timestamps, candidate and in-state reminders start again.
fn enter(d: &mut Domain, s: &mut Step, to: State, now_ms: i64) {
    d.state = to;
    d.state_changed_at_ms = now_ms;
    d.candidate = None;
    d.candidate_count = 0;
    d.ownership_seen = 0;
    d.reminders_sent.retain(|r| !r.in_state());
    s.to = to;
    if matches!(to, State::Healthy | State::Degraded) {
        s.actions.push(Action::ActivateAddresses);
    }
}

fn remove(d: &mut Domain, s: &mut Step, reason: RemoveReason, now_ms: i64) {
    enter(d, s, State::Removing, now_ms);
    if reason != RemoveReason::Requested {
        s.reason = Some(reason.code());
    }
    s.actions.extend([
        Action::DeletePendingAddresses,
        Action::CreateRemoveJob(reason),
    ]);
}

fn check(d: &mut Domain, c: &Check, now_ms: i64, s: &mut Step) {
    if matches!(d.state, State::Removing | State::Removed) {
        s.refused = Some(Refusal::NotApplicable);
        return;
    }
    s.next_check_ms = Some(CHECK_EVERY_MS);

    // C8: track both resolvers failing; after 6 hours the cd=1 decision stands in for `error`.
    let mut outcomes = c.resolvers;
    if outcomes == [Outcome::Error, Outcome::Error] {
        if cd1_due(d, outcomes, now_ms) {
            match c.cd1 {
                Some(issue) => {
                    outcomes = [Outcome::Fail, Outcome::Fail];
                    s.issues.push(issue.code());
                    if d.platform {
                        s.actions.push(Action::Alert("platform_domain_failing"));
                    }
                }
                None => s.actions.push(Action::RequeryCd1),
            }
        }
        d.both_error_since_ms.get_or_insert(now_ms);
    } else {
        d.both_error_since_ms = None;
    }

    // Time rules first: the unverified expiry, then 14 days in `failing`.
    if matches!(d.state, State::Pending | State::Verifying)
        && d.ownership_verified_at_ms.is_none()
        && d.expiry_anchor()
            .is_some_and(|a| now_ms - a >= FOURTEEN_DAYS_MS)
    {
        s.next_check_ms = None;
        return remove(d, s, RemoveReason::UnverifiedExpired, now_ms);
    }
    if d.state == State::Failing
        && !d.platform
        && d.failing_since_ms
            .is_some_and(|f| now_ms - f >= FOURTEEN_DAYS_MS)
    {
        return suspend(d, s, "failing_14_days", now_ms);
    }

    match d.state {
        State::Pending => pending(d, c, now_ms, s),
        State::Suspended => {
            if c.new_ownership_seen {
                d.ownership_seen += 1;
            } else {
                d.ownership_seen = 0;
            }
            if d.ownership_seen >= 2 {
                d.ownership_verified_at_ms = Some(now_ms);
                enter(d, s, State::Verifying, now_ms);
                s.actions.push(Action::RerecordBaselines);
            } else if d.ownership_seen == 1 {
                s.next_check_ms = Some(CONFIRM_AFTER_MS);
            }
        }
        _ => agree(d, outcomes, now_ms, s),
    }
    if s.to == s.from {
        reminders(d, now_ms, s);
    }
}

fn pending(d: &mut Domain, c: &Check, now_ms: i64, s: &mut Step) {
    if c.ds_record_present {
        s.issues.push("ds_record_present");
    }
    if !d.creates_zone {
        return;
    }
    match c.zone {
        Some(ZoneStatus::Gone) => {
            enter(d, s, State::Removed, now_ms);
            s.reason = Some("zone_expired");
            s.events.push(Event::Removed {
                reason: "zone_expired",
            });
            s.next_check_ms = None;
        }
        Some(ZoneStatus::Active { onboarded }) => {
            d.zone_active_at_ms.get_or_insert(now_ms);
            if onboarded {
                enter(d, s, State::Verifying, now_ms);
                s.next_check_ms = Some(0);
            }
        }
        Some(ZoneStatus::Pending) | None => {}
    }
}

fn suspend(d: &mut Domain, s: &mut Step, reason: &'static str, now_ms: i64) {
    enter(d, s, State::Suspended, now_ms);
    s.reason = Some(reason);
    s.actions.push(Action::NewOwnershipToken);
    s.events.push(Event::Suspended { reason });
}

/// The state an agreed outcome leads to, or `None` when it matches the current state.
fn target(state: State, o: Outcome) -> Option<State> {
    use Outcome as O;
    use State as S;
    match (state, o) {
        (S::Verifying, O::Pass) => Some(S::Healthy),
        (S::Verifying, O::Degraded) => Some(S::Degraded),
        (S::Healthy | S::Failing, O::Degraded) => Some(S::Degraded),
        (S::Degraded | S::Failing, O::Pass) => Some(S::Healthy),
        (S::Healthy | S::Degraded, O::Fail) => Some(S::Failing),
        (S::Healthy | S::Degraded | S::Failing, O::OwnershipChanged(_)) => Some(S::Suspended),
        _ => None,
    }
}

/// Agreement and the two-cycle rule ([Outcome per resolver and agreement] steps 3–4).
///
/// [Outcome per resolver and agreement]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/identity-domains.md
fn agree(d: &mut Domain, outcomes: [Outcome; 2], now_ms: i64, s: &mut Step) {
    let [a, b] = outcomes;
    if a == Outcome::Error || a != b {
        // No agreed outcome: nothing changes.
        s.next_check_ms = Some(RETRY_AFTER_MS);
        return;
    }
    // H9: on the platform domain an ownership-level outcome counts as `fail`.
    let o = match a {
        Outcome::OwnershipChanged(_) if d.platform => Outcome::Fail,
        o => o,
    };
    s.agreed = Some(o);
    let Some(to) = target(d.state, o) else {
        d.candidate = None;
        d.candidate_count = 0;
        return;
    };
    if d.candidate == Some(o) {
        d.candidate_count = d.candidate_count.saturating_add(1);
    } else {
        d.candidate = Some(o);
        d.candidate_count = 1;
    }
    if d.candidate_count < 2 {
        s.next_check_ms = Some(CONFIRM_AFTER_MS);
        return;
    }
    let from = d.state;
    if let Outcome::OwnershipChanged(r) = o {
        return suspend(d, s, r.code(), now_ms);
    }
    enter(d, s, to, now_ms);
    match (from, to) {
        (State::Verifying, State::Healthy) => {
            d.ownership_verified_at_ms = Some(now_ms);
            s.events.push(Event::Verified);
        }
        (State::Verifying, State::Degraded) => {
            d.ownership_verified_at_ms = Some(now_ms);
            s.events.push(Event::Degraded);
        }
        (_, State::Failing) => {
            d.failing_since_ms = Some(now_ms);
            s.events.push(Event::Failing);
            if d.platform {
                s.actions.push(Action::Alert("platform_domain_failing"));
            }
        }
        (State::Failing, _) => {
            d.failing_since_ms = None;
            s.events.push(if to == State::Healthy {
                Event::Recovered { from }
            } else {
                Event::Degraded
            });
        }
        (_, State::Degraded) => s.events.push(Event::Degraded),
        (_, _) => s.events.push(Event::Recovered { from }),
    }
}

fn reminders(d: &mut Domain, now_ms: i64, s: &mut Step) {
    let mut due = Vec::new();
    if matches!(
        d.state,
        State::Pending | State::Verifying | State::Degraded | State::Failing | State::Suspended
    ) {
        let in_state = now_ms - d.state_changed_at_ms;
        for (r, at) in [
            (Reminder::Hours24, DAY),
            (Reminder::Hours72, 3 * DAY),
            (Reminder::Days7, 7 * DAY),
        ] {
            if in_state >= at {
                due.push(r);
            }
        }
    }
    if matches!(d.state, State::Pending | State::Verifying)
        && d.ownership_verified_at_ms.is_none()
        && d.expiry_anchor().is_some_and(|a| now_ms - a >= 12 * DAY)
    {
        due.push(Reminder::Day12);
    }
    if d.state == State::Pending
        && d.creates_zone
        && d.zone_active_at_ms.is_none()
        && now_ms - d.created_at_ms >= 21 * DAY
    {
        due.push(Reminder::Day21);
    }
    for r in due {
        if !d.reminders_sent.contains(&r) {
            d.reminders_sent.push(r);
            s.events.push(Event::Reminder(r));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use Outcome::{Degraded as D, Error as E, Fail as F, Pass as P};

    const T0: i64 = 1_800_000_000_000;

    fn domain(state: State, platform: bool) -> Domain {
        let mut d = Domain::new(platform, false, T0);
        d.state = state;
        if !matches!(state, State::Pending | State::Verifying) {
            d.ownership_verified_at_ms = Some(T0);
        }
        if state == State::Failing {
            d.failing_since_ms = Some(T0);
        }
        d
    }

    fn cycle(d: &mut Domain, r: [Outcome; 2], now: i64) -> Step {
        step(d, Input::Check(Check::of(r)), now)
    }

    /// Two agreeing cycles of `o`, five minutes apart; returns the second step.
    fn twice(d: &mut Domain, o: Outcome, now: i64) -> Step {
        let first = cycle(d, [o, o], now);
        assert_eq!(first.to, first.from, "one cycle never changes the state");
        cycle(d, [o, o], now + CONFIRM_AFTER_MS)
    }

    const OWN: Outcome = Outcome::OwnershipChanged(OwnershipReason::NameserversChanged);

    /// Covers: H7
    #[test]
    fn h7_resolver_disagreement() {
        // One resolver erroring: nothing changes, retried in 2 minutes, however often it happens.
        let mut d = domain(State::Healthy, false);
        for i in 0..10 {
            let s = cycle(&mut d, [F, E], T0 + i * RETRY_AFTER_MS);
            assert_eq!((s.to, s.agreed), (State::Healthy, None));
            assert_eq!(s.next_check_ms, Some(RETRY_AFTER_MS));
        }
        assert_eq!(d.candidate, None);
        // One resolver lying: no agreed outcome either.
        let s = cycle(&mut d, [F, P], T0);
        assert_eq!((s.to, s.agreed), (State::Healthy, None));
        assert_eq!(d.candidate, None);

        // Two consecutive agreeing cycles change it; the first schedules the confirming check.
        let s = cycle(&mut d, [F, F], T0);
        assert_eq!(
            (s.to, s.next_check_ms),
            (State::Healthy, Some(CONFIRM_AFTER_MS))
        );
        assert_eq!((d.candidate, d.candidate_count), (Some(F), 1));
        let s = cycle(&mut d, [F, F], T0 + CONFIRM_AFTER_MS);
        assert_eq!(s.to, State::Failing);
        assert_eq!(s.events, vec![Event::Failing]);
        assert_eq!(d.failing_since_ms, Some(T0 + CONFIRM_AFTER_MS));

        // A different agreed outcome resets the count; one matching the state clears it.
        let mut d = domain(State::Healthy, false);
        cycle(&mut d, [F, F], T0);
        cycle(&mut d, [D, D], T0 + 1);
        assert_eq!((d.candidate, d.candidate_count), (Some(D), 1));
        cycle(&mut d, [P, P], T0 + 2);
        assert_eq!((d.candidate, d.candidate_count), (None, 0));
        assert_eq!(d.state, State::Healthy);

        // Both resolvers failing every cycle: no change for 6 hours.
        let mut d = domain(State::Healthy, false);
        let mut now = T0;
        while now < T0 + BOTH_ERROR_ESCALATION_MS {
            assert!(!cd1_due(&d, [E, E], now));
            let s = cycle(&mut d, [E, E], now);
            assert_eq!((s.to, s.agreed), (State::Healthy, None));
            assert!(s.actions.is_empty());
            now += RETRY_AFTER_MS;
        }
        assert_eq!(d.both_error_since_ms, Some(T0));
        // At 6 hours the monitor re-queries with cd=1; without the answer nothing changes.
        assert!(cd1_due(&d, [E, E], now));
        let s = cycle(&mut d, [E, E], now);
        assert_eq!(
            (s.to, s.actions.clone()),
            (State::Healthy, vec![Action::RequeryCd1])
        );
        // A name answering with cd=1 is dnssec_bogus, one still failing dns_unresolvable.
        assert_eq!(cd1_issue(true), DnsIssue::DnssecBogus);
        assert_eq!(cd1_issue(false), DnsIssue::DnsUnresolvable);
        assert_eq!(DnsIssue::DnssecBogus.code(), "dnssec_bogus");
        assert_eq!(DnsIssue::DnsUnresolvable.code(), "dns_unresolvable");
        // The decision makes both resolvers `fail`: two cycles later the domain is failing.
        let with_cd1 = |d: &mut Domain, issue, now| {
            let mut c = Check::of([E, E]);
            c.cd1 = Some(issue);
            step(d, Input::Check(c), now)
        };
        let s = with_cd1(&mut d, DnsIssue::DnssecBogus, now);
        assert_eq!(
            (s.agreed, s.issues.clone()),
            (Some(F), vec!["dnssec_bogus"])
        );
        assert_eq!(s.to, State::Healthy);
        let s = with_cd1(&mut d, DnsIssue::DnssecBogus, now + CONFIRM_AFTER_MS);
        assert_eq!(s.to, State::Failing);
        assert_eq!(s.events, vec![Event::Failing]);
        // Any cycle with a resolver answering clears the escalation clock.
        cycle(&mut d, [P, E], now + 2 * CONFIRM_AFTER_MS);
        assert_eq!(d.both_error_since_ms, None);

        // dns_unresolvable on the platform domain pages.
        let mut p = domain(State::Healthy, true);
        cycle(&mut p, [E, E], T0);
        let s = with_cd1(
            &mut p,
            DnsIssue::DnsUnresolvable,
            T0 + BOTH_ERROR_ESCALATION_MS,
        );
        assert_eq!(s.issues, vec!["dns_unresolvable"]);
        assert!(
            s.actions
                .contains(&Action::Alert("platform_domain_failing"))
        );

        // A nameservers domain whose parent still publishes DS: ds_record_present while pending.
        let mut n = Domain::new(false, true, T0);
        let mut c = Check::of([P, P]);
        c.zone = Some(ZoneStatus::Pending);
        c.ds_record_present = true;
        let s = step(&mut n, Input::Check(c), T0 + HOUR);
        assert_eq!(
            (s.to, s.issues),
            (State::Pending, vec!["ds_record_present"])
        );
    }

    #[test]
    fn transition_table() {
        use State::*;

        // pending + Init: zone active (or external) → verifying with a check now; else stays pending.
        let mut d = Domain::new(false, false, T0);
        let s = step(&mut d, Input::Init { zone_active: true }, T0);
        assert_eq!((s.to, s.actions), (Verifying, vec![Action::CheckNow]));
        let mut z = Domain::new(false, true, T0);
        assert_eq!(
            step(&mut z, Input::Init { zone_active: false }, T0).to,
            Pending
        );

        // pending + check, created zone: still pending (reminders), active and onboarded → verifying,
        // active with onboarding unfinished → pending, gone → removed (zone_expired).
        let zone_check = |z: &mut Domain, st, now| {
            let mut c = Check::of([P, P]);
            c.zone = Some(st);
            step(z, Input::Check(c), now)
        };
        assert_eq!(
            zone_check(&mut z, ZoneStatus::Pending, T0 + HOUR).to,
            Pending
        );
        let s = zone_check(
            &mut z,
            ZoneStatus::Active { onboarded: false },
            T0 + 2 * DAY,
        );
        assert_eq!(s.to, Pending);
        assert_eq!(z.zone_active_at_ms, Some(T0 + 2 * DAY));
        let s = zone_check(&mut z, ZoneStatus::Active { onboarded: true }, T0 + 3 * DAY);
        assert_eq!(s.to, Verifying);
        let mut g = Domain::new(false, true, T0);
        let s = zone_check(&mut g, ZoneStatus::Gone, T0 + 28 * DAY);
        assert_eq!((s.to, s.reason), (Removed, Some("zone_expired")));
        assert_eq!(
            s.events,
            vec![Event::Removed {
                reason: "zone_expired"
            }]
        );

        // pending, verifying + check, never verified, 14 days → removing (unverified_expired). For a
        // created zone the 14 days count from activation.
        for st in [Pending, Verifying] {
            let mut d = domain(st, false);
            assert_eq!(cycle(&mut d, [F, F], T0 + FOURTEEN_DAYS_MS - 1).to, st);
            let s = cycle(&mut d, [F, F], T0 + FOURTEEN_DAYS_MS);
            assert_eq!((s.to, s.reason), (Removing, Some("unverified_expired")));
            assert_eq!(
                s.actions,
                vec![
                    Action::DeletePendingAddresses,
                    Action::CreateRemoveJob(RemoveReason::UnverifiedExpired)
                ]
            );
        }
        assert_eq!(z.state, Verifying);
        assert_eq!(cycle(&mut z, [F, F], T0 + 15 * DAY).to, Verifying);
        assert_eq!(cycle(&mut z, [F, F], T0 + 16 * DAY).to, Removing);

        // pending, verifying + eviction: never verified → removing (evicted); a verified holder never.
        for st in [Pending, Verifying] {
            let mut d = domain(st, false);
            let s = step(&mut d, Input::Evict, T0);
            assert_eq!((s.to, s.reason), (Removing, Some("evicted")));
            assert_eq!(
                s.actions,
                vec![
                    Action::DeletePendingAddresses,
                    Action::CreateRemoveJob(RemoveReason::Evicted),
                    Action::AuditEvict
                ]
            );
        }
        let mut v = domain(Healthy, false);
        assert_eq!(
            step(&mut v, Input::Evict, T0).refused,
            Some(Refusal::NotApplicable)
        );
        let mut v = domain(Verifying, false);
        v.ownership_verified_at_ms = Some(T0);
        assert_eq!(step(&mut v, Input::Evict, T0).to, Verifying);

        // The agreed-outcome rows, each after two cycles.
        let rows: &[(State, Outcome, State, Vec<Event>)] = &[
            (Verifying, P, Healthy, vec![Event::Verified]),
            (Verifying, D, Degraded, vec![Event::Degraded]),
            (Verifying, F, Verifying, vec![]),
            (Verifying, OWN, Verifying, vec![]),
            (Healthy, D, Degraded, vec![Event::Degraded]),
            (Healthy, F, Failing, vec![Event::Failing]),
            (Degraded, F, Failing, vec![Event::Failing]),
            (
                Degraded,
                P,
                Healthy,
                vec![Event::Recovered { from: Degraded }],
            ),
            (
                Failing,
                P,
                Healthy,
                vec![Event::Recovered { from: Failing }],
            ),
            (Failing, D, Degraded, vec![Event::Degraded]),
        ];
        for (from, o, to, events) in rows {
            let mut d = domain(*from, false);
            let s = twice(&mut d, *o, T0 + HOUR);
            assert_eq!((s.to, &s.events), (*to, events), "{from:?} + {o:?}");
            if from == &Verifying && to != &Verifying {
                assert_eq!(
                    d.ownership_verified_at_ms,
                    Some(T0 + HOUR + CONFIRM_AFTER_MS)
                );
            }
            if matches!(to, Healthy | Degraded) {
                assert!(s.actions.contains(&Action::ActivateAddresses));
            }
            if from == &Failing {
                assert_eq!(d.failing_since_ms, None);
            }
        }

        // healthy, degraded, failing + ownership_changed ×2 → suspended with the reason and a new token.
        for (from, r) in [
            (Healthy, OwnershipReason::NameserversChanged),
            (Degraded, OwnershipReason::OwnershipRecordMissing),
            (Failing, OwnershipReason::RegistrationChanged),
        ] {
            let mut d = domain(from, false);
            let s = twice(&mut d, Outcome::OwnershipChanged(r), T0 + HOUR);
            assert_eq!((s.to, s.reason), (Suspended, Some(r.code())));
            assert_eq!(s.events, vec![Event::Suspended { reason: r.code() }]);
            assert_eq!(s.actions, vec![Action::NewOwnershipToken]);
        }

        // failing for 14 days → suspended (failing_14_days).
        let mut d = domain(Failing, false);
        assert_eq!(cycle(&mut d, [F, F], T0 + FOURTEEN_DAYS_MS - 1).to, Failing);
        let s = cycle(&mut d, [F, F], T0 + FOURTEEN_DAYS_MS);
        assert_eq!((s.to, s.reason), (Suspended, Some("failing_14_days")));
        assert_eq!(
            s.events,
            vec![Event::Suspended {
                reason: "failing_14_days"
            }]
        );

        // The platform domain: never suspended. Ownership counts as fail: healthy or degraded →
        // failing with the page; failing stays failing; 14 days in failing stays failing.
        for from in [Healthy, Degraded] {
            let mut p = domain(from, true);
            let s = twice(&mut p, OWN, T0 + HOUR);
            assert_eq!((s.to, s.agreed), (Failing, Some(F)));
            assert!(
                s.actions
                    .contains(&Action::Alert("platform_domain_failing"))
            );
        }
        let mut p = domain(Failing, true);
        assert_eq!(twice(&mut p, OWN, T0 + HOUR).to, Failing);
        assert_eq!(cycle(&mut p, [F, F], T0 + 30 * DAY).to, Failing);
        // Its reprove re-reads the zone; DELETE is 403.
        let s = step(&mut p, Input::Reprove, T0);
        assert_eq!(s.actions, vec![Action::RereadZone, Action::CheckNow]);
        let del = Input::Delete {
            live_addresses: false,
            job_failed: false,
        };
        assert_eq!(
            step(&mut p, del.clone(), T0).refused,
            Some(Refusal::ScopeDenied)
        );

        // suspended + reprove: new token, records updated, check now; then the new TXT on both
        // resolvers in two consecutive cycles → verifying with the baselines re-recorded.
        let mut d = domain(Suspended, false);
        let s = step(&mut d, Input::Reprove, T0);
        assert_eq!(
            (s.to, s.actions),
            (
                Suspended,
                vec![
                    Action::NewOwnershipToken,
                    Action::UpdateRecords,
                    Action::CheckNow
                ]
            )
        );
        let seen = |d: &mut Domain, yes, now| {
            let mut c = Check::of([OWN, OWN]);
            c.new_ownership_seen = yes;
            step(d, Input::Check(c), now)
        };
        assert_eq!(seen(&mut d, true, T0 + 1).to, Suspended);
        assert_eq!(seen(&mut d, false, T0 + 2).to, Suspended);
        assert_eq!(seen(&mut d, true, T0 + 3).to, Suspended);
        let s = seen(&mut d, true, T0 + 4);
        assert_eq!(s.to, Verifying);
        assert!(s.actions.contains(&Action::RerecordBaselines));
        assert_eq!(d.ownership_verified_at_ms, Some(T0 + 4));
        // Reprove anywhere else does not apply.
        assert_eq!(
            step(&mut domain(Healthy, false), Input::Reprove, T0).refused,
            Some(Refusal::NotApplicable)
        );

        // any except removing, removed + DELETE: live addresses → 409; otherwise → removing.
        for st in [Pending, Verifying, Healthy, Degraded, Failing, Suspended] {
            let mut d = domain(st, false);
            let busy = Input::Delete {
                live_addresses: true,
                job_failed: false,
            };
            assert_eq!(step(&mut d, busy, T0).refused, Some(Refusal::DomainInUse));
            let s = step(&mut d, del.clone(), T0);
            assert_eq!(s.to, Removing, "{st:?}");
            assert_eq!(
                s.actions,
                vec![
                    Action::StorePendingRules,
                    Action::DeletePendingAddresses,
                    Action::CreateRemoveJob(RemoveReason::Requested)
                ]
            );
        }

        // removing: job failed → alert; daily restart or DELETE with the job failed → restart, 202;
        // job completed → removed with the job's reason.
        let mut d = domain(Removing, false);
        let s = step(&mut d, Input::JobFailed, T0);
        assert_eq!(
            (s.to, s.actions),
            (Removing, vec![Action::Alert("domain_remove_failed")])
        );
        let s = step(&mut d, Input::DailyRestart { job_failed: true }, T0);
        assert_eq!(s.actions, vec![Action::RestartRemoveJob]);
        let s = step(
            &mut d,
            Input::Delete {
                live_addresses: false,
                job_failed: true,
            },
            T0,
        );
        assert_eq!(
            (s.to, s.actions),
            (Removing, vec![Action::RestartRemoveJob])
        );
        assert_eq!(cycle(&mut d, [P, P], T0).to, Removing);
        for r in [
            RemoveReason::Requested,
            RemoveReason::Evicted,
            RemoveReason::UnverifiedExpired,
        ] {
            let mut d = domain(Removing, false);
            let s = step(&mut d, Input::JobCompleted(r), T0);
            assert_eq!(
                (s.to, s.events),
                (Removed, vec![Event::Removed { reason: r.code() }])
            );
        }
        let mut gone = domain(Removed, false);
        assert_eq!(
            step(&mut gone, del, T0).refused,
            Some(Refusal::NotApplicable)
        );

        // Reminders at 24 h, 72 h and 7 days in the state, once each, reset by a state change; day 12
        // for a never-verified domain.
        let mut d = domain(Verifying, false);
        let mut sent = Vec::new();
        for h in (1..=13 * 24).step_by(1) {
            let s = cycle(&mut d, [F, F], T0 + h * HOUR);
            for e in s.events {
                if let Event::Reminder(r) = e {
                    sent.push((h, r));
                }
            }
        }
        assert_eq!(
            sent,
            vec![
                (24, Reminder::Hours24),
                (72, Reminder::Hours72),
                (168, Reminder::Days7),
                (288, Reminder::Day12)
            ]
        );
        let mut d = domain(Healthy, false);
        assert!(cycle(&mut d, [P, P], T0 + 8 * DAY).events.is_empty());
        let t = T0 + 8 * DAY;
        twice(&mut d, D, t);
        let s = cycle(&mut d, [D, D], t + CONFIRM_AFTER_MS + DAY);
        assert_eq!(s.events, vec![Event::Reminder(Reminder::Hours24)]);
        twice(&mut d, P, t + 2 * DAY);
        twice(&mut d, D, t + 3 * DAY);
        let s = cycle(&mut d, [D, D], t + 4 * DAY + CONFIRM_AFTER_MS);
        assert_eq!(s.events, vec![Event::Reminder(Reminder::Hours24)]);

        // A created zone still pending gets the final reminder at day 21.
        let mut n = Domain::new(false, true, T0);
        n.reminders_sent = vec![Reminder::Hours24, Reminder::Hours72, Reminder::Days7];
        let s = zone_check(&mut n, ZoneStatus::Pending, T0 + 21 * DAY);
        assert_eq!(s.events, vec![Event::Reminder(Reminder::Day21)]);
    }
}
