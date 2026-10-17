# Calendar invitations

Binding for implementation. Pylota Mail transports and verifies iCalendar; it does not keep a calendar
(parity decision of 2026-10-17). Inbound, it finds every calendar part of a message, parses it under
bounds, checks that the authenticated sender is entitled to send that iTIP method, and exposes a bounded,
structured `calendar` object on the message and a thin `calendar.received` event. Outbound, it sends
correct iMIP (RFC 6047) built from an iCalendar payload the caller supplies (for Pylota, produced by
Kairos), and it offers one convenience route that answers a verified invitation. Calendar state (events,
recurrence expansion, agendas, SEQUENCE ordering, conflicts and reminders) belongs to the consumer. One
store per calendar fact: a second calendar inside the mail service would have to be kept consistent with
the consumer's, and for Pylota that consumer, Kairos, already arbitrates conflicts in PostgreSQL.

This page replaces the six-string summary in `automated_json.calendar` that
[Inbound › Parsing](inbound.md#parsing) specified for edge row [B8](../edge-cases.md).

| | |
|---|---|
| Requirements | FR-CAL-1 … FR-CAL-14 (this page); FR-IN-6 (classification), FR-OUT-3, FR-OUT-13, FR-THR-1 (extended), FR-PRV-3 (erasure) |
| Edge cases | [U1](../edge-cases.md)–[U55](../edge-cases.md) (this page); [B8](../edge-cases.md) (rewritten) |
| Code | `crates/core/src/ical/{mod.rs, lex.rs, value.rs, rrule.rs, tz.rs, windows_zones.rs, vtimezone.rs, write.rs}` (the bounded iCalendar reader and writer; pure; `windows_zones.rs` is generated), `crates/core/src/calendar.rs` (part selection, the calendar object, verification, outbound validation and normalisation, the REPLY builder; pure), `crates/core/src/compose.rs` (the iMIP MIME structure), `crates/worker/src/consumers/inbound.rs` (step 10a and the step 12 query), `mailbox/{ingest.rs, calendar.rs, submit.rs, read.rs}` (UID threading, `organized_here`, the stale-SEQUENCE guard, the reply checks, the event), `handlers/{send.rs, calendar_reply.rs}`, `transport/cloudflare.rs` (the raw path), `webhooks/payloads.rs` (`calendar_received`), `console/pages/thread.rs` (the calendar panel), `mcp/tools.rs`, `crates/cli/src/commands/{send.rs, calendar_reply.rs}`, `xtask/src/cldr.rs` (`cargo xtask cldr-windows-zones`), `fuzz/fuzz_targets/ical_parse.rs`, `crates/conformance/corpus/calendar/` |
| Tables | Mailbox `messages.calendar_json` and `messages.calendar_uid_hash` with the partial index `messages_calendar_uid` (mailbox schema v1, [§4.1](#41-columns-and-caps)); `automated_json.calendar` is removed. No D1 table |
| Contracts | `POST /v1/identities/{identity_id}/messages/{message_id}/calendar-reply` (`messages:send`, `Idempotency-Key` required); the `calendar` request field on send and reply (and on drafts, [Drafts](drafts.md)); the `calendar` field of the Message object; the event `calendar.received`; the error codes `calendar_invalid`, `calendar_too_large`, `calendar_organizer_mismatch`, `calendar_attendee_mismatch`, `calendar_recipient_mismatch`, `calendar_sequence_stale`, `calendar_not_invitation`, `calendar_not_verified`, `calendar_cancelled`, `calendar_superseded`, `calendar_event_past`, `calendar_not_attendee`, `calendar_attendee_address_unavailable`, and the new `details.reason` values `calendar_needs_raw_mime` (`422 transport_unavailable`) and `calendar_single_recipient` (`400 too_many_recipients`). No new permission and no new policy field |
| Limits | [§11](#11-billing-and-limits); [Limits › Calendar](../../reference/limits.md#calendar) after integration |
| Built in | [M32](../build-plan.md#m32--calendar-invitations-after-m7-m8-m9-m14-m15-m16-and-m21) |

## 1. Model

```text
 inbound                                                        outbound
 ───────                                                        ────────
 pm-inbound consumer                                            POST …/messages, …/reply, a draft send
   step 10  attachments, TNEF unpacked                            { calendar: { ics } }
   step 10a calendar parts ─▶ core::ical (bounded parse)            │ policy step 9a: kind, operation, size,
            ─▶ calendar object ─▶ verification                      │   parse, iTIP structure, normalise
   step 12  the identity's addresses (is_self)                      │ policy step 13a: ORGANIZER or ATTENDEE
        │ IngestInput.calendar                                      │   = the resolved From; recipients
        ▼                                                           ▼
 IdentityMailbox.ingest                                         IdentityMailbox.submit
   UID thread join (verified only) · organized_here               stale-SEQUENCE guard (and again in the
   messages.calendar_json · calendar_uid_hash                     accept transaction)
   outbox: calendar.received   (only when status = received)      compose: multipart/alternative with
        │                                                         text/calendar; method=…; charset=UTF-8
        ▼                                                           │
 Fanout ─▶ webhooks, event feed, stream                         pm-outbound: never falls back to the
          (needs messages:read, J29)                            platform domain; Cloudflare: raw MIME
                                                                  
 POST …/messages/{message_id}/calendar-reply ─▶ checks (verified REQUEST, attendee, not cancelled,
   superseded or past) ─▶ iTIP REPLY to the ORGANIZER, from the attendee address, in the same thread
```

Terms used on this page:

- **Calendar part**: a MIME leaf of the outer message whose declared type is `text/calendar` or
  `application/ics`, or whose filename ends in `.ics`, or whose sniffed type is calendar
  (`BEGIN:VCALENDAR`, [Inbound › Sniffing](inbound.md#sniffing)), or such a file extracted from TNEF.
- **Calendar object**: one parsed `VCALENDAR`, the unit this page verifies and stores.
- **Primary component**: the `VEVENT` of the object that this page describes at the top level of the
  `calendar` field; the other `VEVENT`s with the same `UID` are **overrides** ([§2.4](#24-primary-component-and-overrides)).
- **Authenticated sender**: the `From` address when its domain is authenticated ([§3.1](#31-the-authenticated-sender)).

What Pylota Mail never does: answer, accept or decline an invitation on its own (the service still never
auto-answers, [B8](../edge-cases.md)); expand a recurrence; keep events, an agenda or reminders; resolve
SEQUENCE conflicts between two messages for the consumer; fetch a `TZURL`, an `ATTACH` URI or any other URL
in an object ([Security § 8.4](security.md#84-no-remote-fetch)).

## 2. Inbound: finding and reading calendar parts

Edge rows: U1–U16.

### 2.1 Which parts are read

`core::calendar::select_parts(parsed: &ParsedMessage, attachments: &[AttachmentMeta]) -> Vec<PartRef>`
walks the **outer** message's leaves in MIME order (depth first, the order of
[Inbound › Parsing](inbound.md#parsing)), then the files that `core::mime::tnef::extract` took out of a TNEF
part, and keeps every calendar part, at most 10 per message (a Pylota Mail choice; later parts stay
ordinary attachments and the warning `parts_skipped` is recorded):

1. A part inside a nested `message/rfc822` (a forwarded message, or the returned copy inside a DSN) is never
   read. Only the outer message's own invitation is this message's calendar ([FR-THR-2](../prd.md), U14).
2. Messages classified `dsn` or `mdn` are not read at all, so a bounce of our own invitation never looks
   like an invitation (U14).
3. Each kept part is decoded (transfer encoding, then the declared `charset`, default UTF-8 as RFC 5545
   §3.1.4 states) and parsed on its own ([§2.2](#22-the-bounded-reader)).
4. Parts whose objects are **the same**, meaning equal `METHOD`, equal set of `(UID, RECURRENCE-ID,
   SEQUENCE, DTSTAMP)` over their `VEVENT`s, are one object. Google and other senders put the same object
   in the `multipart/alternative` and again as an `invite.ics` attachment; it is counted once (U1).
5. The object that becomes the message's `calendar` is the first one in this order: a `text/calendar` part
   that carries a `method` parameter (an iMIP body part, RFC 6047 §2.4); then any other calendar part in
   MIME order; then a calendar file from TNEF. Every further distinct object is counted in
   `other_objects`, with the warning `other_objects`, and is left as an attachment (U13). RFC 6047 §2.4
   lets several iMIP parts with different methods sit in `multipart/mixed`; no sender this page knows of
   does that, and one object per message keeps the event and the reply route unambiguous.
6. A TNEF stream whose `attMessageClass` starts with `IPM.Schedule.Meeting.` but that carries no calendar
   file is a meeting written as MAPI properties: Microsoft's MIME writers map that class to `text/calendar`
   for pure MIME messages and copy it to `attMessageClass` inside TNEF ([MS-OXCMAIL › Message Class](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmail/d9e7eb99-e68d-4796-90d8-6e2fbc883a90),
   updated 2025-05-20, read 2026-10-17). Converting MAPI meeting properties to iCalendar
   ([MS-OXCICAL](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcical/a685a040-5b69-4c84-b084-795113fb4012),
   protocol revision 15.0 of 2025-05-20, read 2026-10-17) is out of scope for v1, so the message gets no
   `calendar` object and the flag `tnef_meeting_not_converted` in `flags_json` (U13).

The chosen part's `attachment_id` is recorded: every calendar part is already an attachment row, because
every leaf that is not the selected text or HTML body is one ([Inbound › Parsing](inbound.md#parsing)),
so the consumer can always fetch the exact bytes with `attachments:read`. Calendar attachments are not
risky, are never text-extracted (their family is not in `inbound.extract_attachment_text`) and follow the
message's visibility.

The system identity's mailbox stores the object like any other but never emits `calendar.received`
([§4.4](#44-the-calendarreceived-event)).

### 2.2 The bounded reader

`core::ical` is Pylota Mail's own reader and writer for the subset of RFC 5545 that this page uses. The
obvious crate, `calcard` 0.3.14 (published 2026-09-15, crates.io metadata read 2026-10-17), was refused:
it depends on `chrono` and `ahash` with default features, which are `clock`, `std`, `oldtime` and
`wasmbind` for `chrono` 0.4.45 and `std` and `runtime-rng` (`getrandom`) for `ahash` 0.8.12 (crates.io
metadata read 2026-10-17). `core` must not read the clock or randomness and must not depend on
`wasm-bindgen` or `js-sys` ([Rust workspace § 2](rust-workspace.md#2-crate-responsibilities-and-allowed-dependencies)),
and Cargo unifies features, so the crate would break that rule for the whole crate graph. `core::ical`
uses only `chrono` and `chrono-tz`, already pinned without default features.

**Content lines** (RFC 5545 §3.1, read 2026-10-17). Lines end in CRLF; a bare LF is accepted as a line end
on input. Unfolding removes a line break followed by one space or tab, and works on bytes, so a fold
inside a multi-octet UTF-8 sequence is restored before decoding, as §3.1 requires. Each content line is
`name *(";" param) ":" value`; a parameter value containing `:`, `;` or `,` must be a quoted string and
never contains `"`. Text values are unescaped per §3.3.11 (`\\`, `\;`, `\,`, `\n` and `\N`); a backslash
before any other character is kept as the character. Names are case-insensitive. Invalid UTF-8 is
replaced with U+FFFD and recorded as the warning `invalid_utf8`.

**Bounds.** Every bound is checked while reading, before anything is allocated for the next item, so the
work per object is linear in its size. All are Pylota Mail choices, sized from the objects real senders
produce (a few kilobytes) and from Kairos's own bounds, so anything this reader accepts Kairos can also
read: Kairos refuses iCalendar input over 2,000,000 bytes or with more than 2,000 `VEVENT`s, 1,000
exceptions per series or 100 attendees (`packages/kairos/src/sync.ts`, read 2026-10-17).

| Bound | Value | When exceeded |
|---|---|---|
| Decoded part size | 1 MiB (1,048,576 bytes) | Not parsed: `object_status: "too_large"` |
| Content lines per object | 20,000 | `object_status: "too_complex"` |
| One unfolded content line | 64 KiB | `too_complex` |
| Parameters per property; one parameter value | 50; 1,024 bytes | `too_complex` |
| Components of any kind per object | 500 | `too_complex` |
| `VEVENT`s with the primary's `UID` | 1 primary and 1,000 overrides parsed; 100 overrides kept in the object | Beyond 1,000: `too_complex`; beyond 100: `overrides_truncated`, `overrides_total` counts all |
| `ATTENDEE` properties per component | 1,000 parsed; 100 kept | Beyond 1,000: `too_complex`; beyond 100: `attendees_truncated` ([§2.7](#27-parties)) |
| `VTIMEZONE`s; observances per `VTIMEZONE`; `RDATE` values per observance | 20; 50; 100 | That `VTIMEZONE` is unusable: warning `vtimezone_unsupported` ([§2.5](#25-times-and-time-zones)) |
| `EXDATE` or `RDATE` values per component | 10,000 each | `too_complex` |
| Nesting (`VCALENDAR` › `VEVENT` › `VALARM`) | 3 levels | `too_complex` |
| `UID` | 1,024 bytes (RFC 5545 §3.8.4.7 requires implementations to accept at least 255 octets) | `object_status: "invalid"` |
| Distinct organizer, attendee and `SENT-BY` addresses over all kept components | 1,000 | `too_complex` (the `parties` list of [§10](#10-privacy) must be complete) |

`object_status` is `parsed`, `invalid` (a syntax error: a missing `END`, a `BEGIN:VCALENDAR` that never
closes, a date that does not parse, a missing `UID`), `too_large`, `too_complex` or `unsupported` (no
`VEVENT`, [§2.3](#23-components-and-methods)). For every status other than `parsed` the object carries only
`object_status`, `method` (when the `METHOD` line was read), `attachment_id`, `verified: false` and its
`reason`; the message itself is stored and delivered as usual (U10, U11).

### 2.3 Components and methods

**Methods.** `METHOD` (at most one; a second `METHOD` line makes the object `invalid`) is classified as
RFC 5546 §1.4 and §3.2 define it for `VEVENT` (read 2026-10-17):

| `method` | `METHOD` | Sent by | Meaning |
|---|---|---|---|
| `publish` | `PUBLISH` | Organizer | Publishes an event; `ATTENDEE` must not be present (§3.2.1) |
| `request` | `REQUEST` | Organizer (an attendee only when delegating, §3.2.2.3) | Invites, reschedules (a higher `SEQUENCE`, §3.2.2.1) or updates |
| `reply` | `REPLY` | Attendee | One attendee's answer; exactly one `ATTENDEE`, the replier (§3.2.3) |
| `add` | `ADD` | Organizer | Adds instances to an existing recurring event; `SEQUENCE` greater than 0 (§3.2.4) |
| `cancel` | `CANCEL` | Organizer | Cancels the event (no `RECURRENCE-ID`), one instance (`RECURRENCE-ID`), or removes the listed attendees (§3.2.5) |
| `refresh` | `REFRESH` | Attendee | Asks the organizer for the latest version; one `ATTENDEE` (§3.2.6) |
| `counter` | `COUNTER` | Attendee | Proposes changes to the organizer (§3.2.7) |
| `declinecounter` | `DECLINECOUNTER` | Organizer | Refuses a counter-proposal (§3.2.8) |
| `none` | absent | – | An iCalendar file that is not a scheduling message (an exported `.ics`). Verified as `publish` is ([§3.2](#32-rules-by-method)) |
| `unknown` | any other value | – | Never verified (`reason: "method_unknown"`) |

A `text/calendar` part's `method` parameter must equal `METHOD`, ignoring case (RFC 6047 §2.4, read
2026-10-17); a different value gives `reason: "method_mismatch"`. A `text/calendar` part without the
parameter is not an iMIP body part under RFC 6047 §2.4 (Note 2); it is still read, with the warning
`method_param_missing`.

**Components.** `VEVENT`s are read. `VTIMEZONE`s are read for time zones only. `VTODO`, `VJOURNAL` and
`VFREEBUSY` are not read: their names are listed in `ignored_components`, and an object with no `VEVENT`
is `unsupported`, with `reason: "no_vevent"` (U9). Unknown components (`X-…` or IANA tokens) are skipped
whole and counted in `ignored_components` as `other`.

**Removed from what agents see** (U12). `VALARM` sub-components and `ATTACH` properties are dropped from
the `calendar` object and counted in `removed.alarms` and `removed.attachments`: alarms are instructions to
the recipient's own client (an `EMAIL` alarm sends mail to the addresses it lists, RFC 5545 §3.6.6, read
2026-10-17), and an `ATTACH` is a URL or inline bytes from the sender. Both stay in the stored attachment.
Every `X-` property is dropped from the object too (`X-ALT-DESC`, `X-MICROSOFT-CDO-*` and the rest); the
object carries only the properties this page names.

### 2.4 Primary component and overrides

All `VEVENT`s of one object should share one `UID` (RFC 5546 §3.2.2 for `REQUEST`). The reader keeps the
`VEVENT`s whose `UID` equals the first `VEVENT`'s and ignores the others with the warning `uid_mismatch`.
Among the kept ones:

- the **primary component** is the one without `RECURRENCE-ID`, or, when every kept component has one (a
  `CANCEL` or `REQUEST` for single instances, U6), the one with the earliest `RECURRENCE-ID`;
- every other kept component is an **override**, identified by its `RECURRENCE-ID` (whose value type must
  equal the primary's `DTSTART` type, RFC 5545 §3.8.4.4; otherwise that override is dropped with the warning
  `override_type_mismatch`) and its `RANGE=THISANDFUTURE` parameter, the only range RFC 5545 §3.8.4.4
  allows;
- two components with the same `RECURRENCE-ID` keep the one with the higher `SEQUENCE`, then the later
  `DTSTAMP` (RFC 5546 §2.1.5, read 2026-10-17), with the warning `duplicate_override`.

Overrides are sorted by `RECURRENCE-ID` and kept up to 100 ([§2.2](#22-the-bounded-reader)). A Google
recurring invitation with edited instances is a master plus overrides in one object; it is read whole
within these bounds (U8).

### 2.5 Times and time zones

Each `DTSTART`, `DTEND`, `RECURRENCE-ID`, `EXDATE`, `RDATE` and `UNTIL` value has one of the three forms
of RFC 5545 §3.3.4 and §3.3.5 (read 2026-10-17): a `DATE` (`19970714`), a `DATE-TIME` in UTC (ends in `Z`),
or a local `DATE-TIME` with a `TZID` parameter or without one (floating). Every time in the object is
returned as a **time value**:

```json
{ "utc": "2026-10-20T08:00:00Z", "local": "2026-10-20T09:00:00", "date": null,
  "tzid": "GMT Standard Time", "tz": "Europe/London", "tz_resolution": "windows" }
```

| `tz_resolution` | When | `utc` | `tz` |
|---|---|---|---|
| `utc` | The value ends in `Z` | the value | `null` |
| `iana` | The `TZID` is an IANA zone name known to `chrono-tz` 0.10.4 (compared case-insensitively) | computed | the zone |
| `windows` | The `TZID`, trimmed and with runs of spaces collapsed, is a Windows zone name in the CLDR table below | computed | the mapped zone |
| `prefixed` | The `TZID` starts with `/` (a globally unique identifier, RFC 5545 §3.2.19) and its longest `/`-separated suffix is an IANA name, as in `/mozilla.org/20050126_1/Europe/London` or `/citadel.org/20190914_1/Europe/Paris` | computed | the zone |
| `vtimezone` | None of the above, and the object defines that `TZID` in a usable `VTIMEZONE` | computed from the definition | `null` |
| `unknown` | None of the above (warning `unknown_tzid`) | `null` | `null` |
| `floating` | No `TZID` and no `Z` | `null`; `floating: true` on the object | `null` |
| `date` | A `DATE` value; `date` is set, `local` and `utc` are `null`; `all_day: true` when `DTSTART` is a date | `null` | `null` |

**Windows zone names** (U2). `core::ical::windows_zones` is generated by `cargo xtask cldr-windows-zones`
from CLDR's `common/supplemental/windowsZones.xml` at a pinned CLDR release tag, recorded in the generated
file's header. Only the `territory="001"` entries are used: LDML says that for each Windows zone ID
"there must be exactly one `<mapZone>` element with `territory="001"`", the default mapping
([UTS #35 › Windows Zones](https://www.unicode.org/reports/tr35/tr35-dates.html#Windows_Zones), read
2026-10-17). The file read on 2026-10-17 (`mapTimezones otherVersion="7e11800" typeVersion="2021a"`) has 142
such entries, for example `GMT Standard Time` → `Europe/London`, `W. Europe Standard Time` →
`Europe/Berlin`, `Pacific Standard Time` → `America/Los_Angeles`, `Romance Standard Time` → `Europe/Paris`
and `UTC` → `Etc/UTC`; some values are tzdb backward links (`India Standard Time` → `Asia/Calcutta`). The
test `core::ical::windows_zones_table` fails when any mapped value does not parse with
`chrono_tz::Tz::from_str`; the generator then resolves it through the tzdb `backward` file. AgentMail does
not import invitations whose time zones are Windows names ([§20](#20-compared-with-agentmail-read-2026-10-17)).

**IANA wins over a definition.** A `TZID` that resolves by name (`iana`, `windows`, `prefixed`) uses the
tzdb rules even when the object also defines it in a `VTIMEZONE`, because the current tzdb is the better
source for future dates. When the definition's offset at the primary `DTSTART` differs from the tzdb
offset, the warning `tz_definition_mismatch` is recorded (U4).

**`VTIMEZONE` evaluation** (U3). For a `TZID` known only from its definition, `core::ical::vtimezone`
computes the offset in force at a local time as RFC 5545 §3.6.5 defines it: the observance (`STANDARD` or
`DAYLIGHT`) with the latest onset at or before the time. Onsets are the observance's `DTSTART`, its `RDATE`s
(at most 100) and the onsets of its `RRULE`, which must be the subset every real zone definition uses:
`FREQ=YEARLY` with one `BYMONTH` and either one `BYDAY` with an ordinal from −5 to −1 or 1 to 5, or one
`BYMONTHDAY`, and optionally `UNTIL` or `COUNT`. Onsets are generated only for the year of the time and the
year before it, so the work is bounded. Any other rule makes that `VTIMEZONE` unusable: warning
`vtimezone_unsupported`, and the `TZID` resolves as `unknown`.

**Gaps and overlaps** (U4). A local time that occurs twice refers to the first occurrence; a local time
that does not exist is read with the offset in force before the gap (RFC 5545 §3.3.5, read 2026-10-17).

**Ends** (RFC 5545 §3.6.1, read 2026-10-17). `DTEND` is the non-inclusive end. Without `DTEND`, the end is
`DTSTART` plus `DURATION`; without either, a date `DTSTART` lasts one day and a date-time `DTSTART` ends at
the same instant. `end.source` is `dtend`, `duration` or `default`. An end before the start is replaced by
the start, with the warning `end_before_start`. All-day ends stay exclusive dates
(`"date": "2026-10-21"` for a one-day event on 20 October), unlike AgentMail's inclusive all-day ends.

**`ends_at`**, the instant after which the event is over, is what the reply route's "past" check reads
([§6.2](#62-rules-in-order)): the primary's end for a one-off event; for a recurring event, `UNTIL` plus the
primary's duration when the rule has `UNTIL`, the latest `RDATE` plus the duration when there is no
`RRULE`, and `null` (open) when the rule has `COUNT` or neither bound, because computing the last instance
of a `COUNT` rule needs an expansion this service does not do. Dates, floating times and `unknown` zones
are read in the tenant's time zone (`tenants.timezone`) for this purpose, and `ends_at_basis` is then
`tenant_timezone` instead of `utc`.

### 2.6 Recurrence

The primary's `RRULE` is validated, never expanded. `rrule` is
`{ "text", "valid", "freq", "interval", "count", "until" }`, with `text` the rule as received (at most 512
bytes; a longer rule is `valid: false` and its text is cut with `rrule_text_truncated`). `valid` follows
RFC 5545 §3.3.10 (read 2026-10-17): `FREQ` present once with one of its seven values; `COUNT` and `UNTIL`
not both; `UNTIL` of the `DTSTART` value type, in UTC when `DTSTART` is UTC or has a `TZID`; every `BY…`
value inside its range; `BYSECOND`, `BYMINUTE` and `BYHOUR` absent when `DTSTART` is a date; `BYMONTHDAY`
not with `WEEKLY`; `BYYEARDAY` not with `DAILY`, `WEEKLY` or `MONTHLY`; `BYWEEKNO` only with `YEARLY`;
`BYSETPOS` only with another `BY…` part; no unknown part names. As Pylota Mail choices, `INTERVAL` must be
1–1,000, `COUNT` 1–100,000 and each `BY…` list at most 366 values, so that a consumer that expands a valid
rule is not handed a rule designed to exhaust it. `FREQ=SECONDLY`, `MINUTELY` and `HOURLY` are valid but add
the warning `rrule_high_frequency`. A second `RRULE` (which RFC 5545 §3.6.1 says should not occur) is
ignored with the warning `multiple_rrule`. An invalid rule changes nothing else: the object stays `parsed`
and `rrule.valid` is `false` (U7).

`exdates_count` and `rdates_count` count the primary's `EXDATE` and `RDATE` values; the values themselves
are in the stored attachment.

### 2.7 Parties

`ORGANIZER` and `ATTENDEE` values are calendar user addresses, and iMIP requires `mailto:` URIs (RFC 6047
§2.3, read 2026-10-17). `core::calendar::cal_address` normalises a value: strip `mailto:` in any case,
decode percent-encoding, drop a `?…` query (warning `mailto_hfields`), then normalise as an address (IDNA
A-label domain, lower case); the whole address is then compared case-insensitively, as the mail systems
that send invitations treat it. A value that is not a `mailto:` URI (`urn:uuid:…`, `invalid:nomail`), or
that does not parse as an address, gives `address: null` with its `uri_scheme`, and never matches anything.

```json
{ "address": "jo@example.net", "name": "Jo Rivera", "role": "req_participant", "partstat": "needs_action",
  "rsvp": true, "cutype": "individual", "delegated_from": [], "delegated_to": [], "sent_by": null,
  "is_self": false }
```

`role` is `chair`, `req_participant` (the default), `opt_participant`, `non_participant` or `other`;
`partstat` is `needs_action` (the default), `accepted`, `declined`, `tentative`, `delegated` or `other`;
`cutype` is `individual` (the default), `group`, `resource`, `room` or `unknown` (unrecognised values
included); `rsvp` defaults to `false` (RFC 5545 §3.2.3, §3.2.12, §3.2.16, §3.2.17, read 2026-10-17).
`delegated_from`, `delegated_to` and `sent_by` hold normalised addresses (at most 5 each). `is_self` is true
when the address is one of the receiving identity's addresses in any status (an invitation may name an old
address). The organizer is `{ "address", "name", "sent_by", "uri_scheme" }`; an object whose kept components
name different organizers is not verified (`organizer_inconsistent`).

When a component has more than 100 attendees, the object keeps the receiving identity's own entries
first, then the others in order, up to 100, sets `attendees_truncated: true` and counts all of them in
`attendees_total`.

### 2.8 Text fields

`summary` (at most 1,000 characters), `location` (1,024), `description` (an excerpt: the first 2,000
characters of `DESCRIPTION`), `comment` (2,000, from `COMMENT`, used by `REPLY` and `COUNTER`) and every
`name` (`CN`, 256) are unescaped, then cleaned as every agent-facing text is: control characters removed,
and the hidden characters of [Inbound › Hidden text](inbound.md#hidden-text-b11) removed. Removing a tag
character or a bidirectional override, or more than two zero-width characters, adds the message flag
`hidden_text` and the warning `hidden_text` (U12). A cut field ends at a character boundary and sets its
`…_truncated` flag. `status` is `tentative`, `confirmed`, `cancelled` or `null` (RFC 5545 §3.8.1.11, read
2026-10-17).

Every string in the object is **untrusted content**: the API marks it so, MCP returns it as such, and any
model input that contains it fences it with the nonce fence of
[Search § 11.6](search.md#116-fencing-mail-content). The `uid` is sender-chosen text too.

### 2.9 The calendar object

The `calendar` field of the Message object (`null` when the message has no calendar part):

```json
{
  "object_status": "parsed",
  "method": "request",
  "verified": true,
  "verified_by": "organizer",
  "reason": null,
  "authenticated_sender": "service@brightwell.example",
  "uid": "040000008200E00074C5B7101A82E00800000000F0A1C2D3E4B5DC0100000000000000001000000",
  "sequence": 1,
  "dtstamp": "2026-10-17T08:02:11Z",
  "recurrence_id": null,
  "status": "confirmed",
  "summary": "MOT and service – AB12 CDE", "summary_truncated": false,
  "description": "Please leave the V5C in the glovebox…", "description_truncated": false,
  "location": "Brightwell Leeds, Unit 4",
  "start": { "utc": "2026-10-20T08:00:00Z", "local": "2026-10-20T09:00:00", "date": null,
             "tzid": "GMT Standard Time", "tz": "Europe/London", "tz_resolution": "windows" },
  "end":   { "utc": "2026-10-20T09:30:00Z", "local": "2026-10-20T10:30:00", "date": null,
             "tzid": "GMT Standard Time", "tz": "Europe/London", "tz_resolution": "windows", "source": "dtend" },
  "all_day": false, "floating": false,
  "ends_at": "2026-10-20T09:30:00Z", "ends_at_basis": "utc",
  "rrule": null, "exdates_count": 0, "rdates_count": 0,
  "organizer": { "address": "service@brightwell.example", "name": "Brightwell Service Desk", "sent_by": null, "uri_scheme": "mailto" },
  "attendees": [
    { "address": "maintenance.acme@agents.example", "name": "Acme Car Hire", "role": "req_participant",
      "partstat": "needs_action", "rsvp": true, "cutype": "individual", "delegated_from": [],
      "delegated_to": [], "sent_by": null, "is_self": true }
  ],
  "attendees_total": 1, "attendees_truncated": false,
  "comment": null,
  "overrides": [], "overrides_total": 0, "overrides_truncated": false,
  "organized_here": false,
  "attachment_id": "att_01JA9B2C3D4EW7X2M5N6P8R0T1",
  "other_objects": 0,
  "ignored_components": [],
  "removed": { "alarms": 1, "attachments": 0, "x_properties": 3 },
  "warnings": [],
  "parser_version": 1
}
```

| Field | Meaning |
|---|---|
| `verified`, `verified_by`, `reason`, `authenticated_sender` | [§3](#3-authenticity). `verified` is `null` on outbound messages |
| `uid`, `sequence`, `dtstamp`, `recurrence_id` | Of the primary component. `sequence` defaults to 0 (RFC 5545 §3.8.7.4); `dtstamp` is UTC (§3.8.7.2) or `null` when missing (warning `dtstamp_missing`); `recurrence_id` is a time value or `null` |
| `overrides[]` | `{ "recurrence_id", "this_and_future", "sequence", "status", "start", "end", "summary", "location", "self" }`: `summary` and `location` only when they differ from the primary's (each at most 256 characters), and `self` the receiving identity's own attendee entry in that override, if any |
| `organized_here` | The identity has sent a `REQUEST`, `ADD` or `PUBLISH` with this `UID` that is not `canceled`, `rejected`, `failed` or `suppressed` ([§4.2](#42-ingest)): an answer or counter-proposal to an invitation this identity sent |
| `warnings[]` | Any of `parts_skipped`, `other_objects`, `invalid_utf8`, `method_param_missing`, `uid_mismatch`, `override_type_mismatch`, `duplicate_override`, `overrides_truncated`, `attendees_truncated`, `unknown_tzid`, `tz_definition_mismatch`, `vtimezone_unsupported`, `end_before_start`, `dtstamp_missing`, `multiple_rrule`, `rrule_high_frequency`, `rrule_text_truncated`, `mailto_hfields`, `hidden_text`. New values may be added within `/v1` |
| `parser_version` | `core::ical::PARSER_VERSION`; a re-parse after a reader fix re-emits the event when the object changed ([§4.4](#44-the-calendarreceived-event)) |

### 2.10 Classification

[Inbound › Automated mail and loops](inbound.md#automated-mail-and-loops) row 5 becomes: "A calendar
object with `object_status: "parsed"` was found ([Calendar invitations §2.1](calendar-invitations.md#21-which-parts-are-read)),
and none of the rows above matched" → `calendar`. Because TNEF is unpacked at consumer step 10, after
classification, step 10a turns a `normal` classification into `calendar` when it finds a parsed object;
it never changes `dsn`, `mdn`, `list` or `automated`. The `calendar` object is set whatever the `kind`, so
an invitation that carries `Auto-Submitted: auto-generated` is `automated` and still has its object.

## 3. Authenticity

Edge rows: U17–U23.

iMIP leaves authentication to S/MIME: "Authentication MUST be performed using S/MIME" and unsigned messages
"may not be trusted" (RFC 6047 §2.2.2, read 2026-10-17). Almost no invitation is signed, and v1 never
verifies S/MIME ([Inbound › Parsing](inbound.md#parsing), B9). Pylota Mail therefore verifies with what it
does check, the domain authentication of every message, and says exactly what it proved. RFC 6047 §2.2.1
says implementations should verify the creator before acting, and that only the organizer may modify or
cancel what she organizes; RFC 5546 §6.1.1 and §6.1.2 name spoofing the organizer and spoofing an attendee
as the threats (read 2026-10-17). This section is how the service tells its consumer which of the two a
message could be.

### 3.1 The authenticated sender

`core::calendar::authenticated_sender(msg) -> Option<Address>` returns the message's `From` address, and
`authenticated_sender` is set, only when all of these hold:

1. the message has exactly one `From` address (`auth_json.multiple_from` is false, [B13](../edge-cases.md));
2. its domain is authenticated: the verdict is `pass` (DMARC pass), or the verdict is `none` (the domain
   publishes no DMARC record) and `auth_json.dmarc.aligned_by` is `dkim` or `spf`, so a passing DKIM
   signature or SPF result aligns with the `From` domain under relaxed alignment
   ([Inbound › Authentication verdict](inbound.md#authentication-verdict), step 6); or the message is a
   test-tenant loopback delivery, whose verdict is `pass` ([L3](../edge-cases.md)).

`unverified`, `fail`, `softfail` and `unaligned` never authenticate a sender (U18). This is the notion the
known-recipient rule already uses for "the authenticated sender being answered"
([Outbound › Recipient filters](outbound.md#recipient-filters)), widened by the aligned-without-a-record case,
because a domain that signs its mail but publishes no DMARC record has still proved the domain.

**What this proves.** DMARC and alignment prove the `From` domain, not the mailbox: a domain whose own mail
system lets one user send as another could forge its own users. This is the same limit every
authentication decision in Pylota Mail has, and it is why `verified_by` and `authenticated_sender` are
returned. On the shared platform domain the service itself guarantees the mailbox: only an identity can
send as its own address, so an invitation from another workspace's identity is verified like any other
(U22).

### 3.2 Rules by method

An object is verified only when `object_status` is `parsed`, the `method` is known, the `text/calendar`
part's `method` parameter (when present) equals `METHOD`, there is at least one `VEVENT`, there is an
authenticated sender, and the rule of its method holds:

| `method` | `verified: true` when the authenticated sender equals | `verified_by` |
|---|---|---|
| `publish`, `request`, `add`, `cancel`, `declinecounter`, `none` | the `ORGANIZER` address; or the organizer's `SENT-BY` address when that address has the same organisational domain as the organizer | `organizer`, or `sent_by` |
| `reply`, `refresh` | the address of the single `ATTENDEE` (an object with zero or several gets `reason: "attendee_count"`); or that attendee's `SENT-BY` under the same domain rule | `attendee`, or `sent_by` |
| `counter` | the address of one of the `ATTENDEE`s (RFC 5546 §3.2.7 lets a counter-proposal list proposed attendees too); or such an attendee's `SENT-BY` under the same domain rule | `attendee`, or `sent_by` |
| `unknown` | never | – |

**`SENT-BY`** (U19). RFC 6047 §2.2.1 and §3 say there is no mechanism to verify that one calendar user may
act for another, and leave the decision to implementations (read 2026-10-17). A `SENT-BY` parameter is
written by the sender, so on its own it proves nothing: `attacker@evil.example` could sign its own mail and
name itself as sending for `ceo@victim.example`. A `SENT-BY` is therefore accepted only when its
organisational domain equals that of the address it acts for, so the domain that authenticated the message
speaks for both mailboxes (an assistant at `corp.example` sending for a manager at `corp.example`). Any
other `SENT-BY` match gives `reason: "sent_by_unverifiable"`.

**Delegates and forwards** (U20, U21). A `REPLY` from a delegate (its `ATTENDEE` has `DELEGATED-FROM`) is
verified as the delegate speaking for itself; whether a delegate is acceptable is the organizer's decision,
which belongs to the consumer, and `delegated_from` is returned for it. A `REQUEST` forwarded by an attendee
(RFC 5546 §3.2.2.6, read 2026-10-17) or sent by a delegator (§3.2.2.3) comes from someone who is not the
organizer; it is not verified, with `reason: "sender_is_delegator"` when the sender is an `ATTENDEE` whose
`DELEGATED-TO` names an address of the receiving identity, and `sender_not_organizer` otherwise.

### 3.3 Reasons

`reason` is `null` when `verified` is true. Otherwise it is the first that applies, in this order:

| `reason` | Meaning |
|---|---|
| `object_invalid`, `object_too_large`, `object_too_complex` | The object could not be read ([§2.2](#22-the-bounded-reader)) |
| `no_vevent` | The object has no `VEVENT` ([§2.3](#23-components-and-methods)) |
| `method_unknown` | `METHOD` is not one of the eight methods |
| `method_mismatch` | The part's `method` parameter differs from `METHOD` |
| `organizer_missing`, `organizer_not_mailto`, `organizer_inconsistent` | No usable `ORGANIZER`, or the kept components name different organizers (U23) |
| `attendee_count` | A `reply` or `refresh` without exactly one `ATTENDEE` |
| `sender_not_authenticated` | No authenticated sender ([§3.1](#31-the-authenticated-sender)) |
| `sender_is_delegator`, `sent_by_unverifiable`, `sender_not_organizer`, `sender_not_attendee` | The authenticated sender is not entitled to send this method ([§3.2](#32-rules-by-method)) |

An unverified object is never hidden: the message is delivered (or quarantined) by the existing rules, its
object says `verified: false` with the reason, and `calendar.received` carries the same. Nothing is
silently left out, unlike AgentMail's invitation import, which leaves the calendar unchanged when its
checks fail ([§20](#20-compared-with-agentmail-read-2026-10-17)).

## 4. Storage, threading and events

Edge rows: U24–U29.

### 4.1 Columns and caps

Two columns are added to `messages` in mailbox schema v1 (the schema is not yet released, so this is no
migration, as for [Service sign-up ledger](service-accounts.md)'s columns):

```sql
-- IdentityMailbox, table messages (mailbox schema v1), added by Calendar invitations
  calendar_json       TEXT,                                -- the calendar object (Calendar invitations §2.9),
                                                           -- at most 262,144 bytes; NULL without a calendar part
  calendar_uid_hash   TEXT,                                -- hex SHA-256 of the primary UID's bytes; NULL unless
                                                           -- object_status = 'parsed'
CREATE INDEX IF NOT EXISTS messages_calendar_uid ON messages(calendar_uid_hash)
  WHERE calendar_uid_hash IS NOT NULL;
```

`automated_json` no longer carries `calendar`; its cap stays 64 KiB. `messages` grows from 45 to 47 columns
(the limit is 100, [Data model › Mailbox notes](data-model.md#mailbox-notes)), and the message insert of
[Inbound › IdentityMailbox.ingest](inbound.md#identitymailboxingest) from 37 to 39 bound parameters (the
limit is 100). No new store exists: the object lives in the message's row, the raw bytes in the message's
attachment, and both go wherever the message goes.

**Cap and shrink order** (a Pylota Mail choice). `calendar_json` is at most 262,144 bytes (256 KiB) of
UTF-8 JSON. The bounds of [§2.2](#22-the-bounded-reader) keep ordinary objects far below it (a Google or
Outlook invitation gives 2–6 KiB of JSON), but 100 attendees with 256-character names and five delegation
addresses each can pass it. When it would, `core::calendar::shrink` removes, in order and only as far as
needed: the `summary` and `location` of every override; overrides from the end (`overrides_truncated`);
attendees beyond the receiving identity's own entries and the first 20 (`attendees_truncated`); the
`description` beyond 500 characters (`description_truncated`). The identifying fields (`method`, `uid`,
`sequence`, `dtstamp`, `recurrence_id`, `verified`, `reason`, times, organizer) and the internal `parties`
list of [§10](#10-privacy) (at most 1,000 entries of 32 hexadecimal characters, about 34 KiB at most)
are never removed.

**Row budget.** `calendar_json` counts in the `fixed` part of the 1,900,000-byte row budget of
[Inbound › Storage caps](inbound.md#storage-caps), which grows from about 270 KiB to about 526 KiB at most.
The cut order is unchanged; with every cap at its maximum, `html_sanitized` keeps at least about 560 KiB
instead of about 820 KiB. The same applies to outbound rows.

### 4.2 Ingest

**Consumer step 10a, "Calendar"** (after step 10, which unpacks TNEF, and before step 11). It runs
`select_parts`, parses the chosen object, computes the verification of [§3](#3-authenticity) from the
authentication verdict of step 7, upgrades a `normal` kind to `calendar` ([§2.10](#210-classification)) and
builds a `CalendarInput`. A reader panic is a parser panic like any other: it aborts the invocation and
the degraded mode of [Inbound › The pm-inbound consumer](inbound.md#the-pm-inbound-consumer) step 4, which
parses headers only from the third attempt, also skips step 10a, so a poison invitation is stored without
its object and never dropped (U10).

**Consumer step 12, "D1 facts"**, gains one query, only when step 10a produced a parsed object:

```sql
SELECT address FROM addresses WHERE identity_id = ?1;   -- every status: an invitation may name an old address
```

and fills `is_self` on the organizer, the attendees and the overrides' `self` entries.

```rust
// crates/worker/src/mailbox/ingest.rs: added to IngestInput
pub calendar: Option<CalendarInput>,

pub struct CalendarInput {
    pub object_json: String,          // the calendar object, already shrunk to 262,144 bytes
    pub uid_hash: Option<String>,     // hex SHA-256 of the primary UID; None unless parsed
    pub method: CalendarMethod,
    pub verified: bool,
    pub summary_for_event: CalendarEventSummary,   // method, uid, sequence, recurrence_id, verified, reason
}
```

**Inside `IdentityMailbox.ingest`** (one transaction, [Inbound › IdentityMailbox.ingest](inbound.md#identitymailboxingest)):

- step 6, **Thread**: `resolve_inbound` receives the calendar UID hash only when `verified` is true
  ([§4.3](#43-threading-by-uid));
- before step 9, **`organized_here`**: one indexed query sets the field in `object_json`:

  ```sql
  SELECT 1 FROM messages
  WHERE calendar_uid_hash = ?1 AND direction = 'outbound'
    AND status NOT IN ('canceled','rejected','failed','suppressed')
    AND json_extract(calendar_json, '$.method') IN ('request','add','publish')
  LIMIT 1;
  ```

- step 9, **Insert**: `calendar_json` and `calendar_uid_hash` are written with the message;
- step 16, **Outbox**: when the stored status is `received` and the mailbox is not the system identity's,
  `calendar.received` is appended after `message.received`, in the same transaction
  ([§4.4](#44-the-calendarreceived-event)).

A duplicate by hash ([B14](../edge-cases.md)) returns the stored message and appends nothing, so a
redelivered invitation emits one event. Two identities that receive one invitation each store their own
copy with their own `is_self` and emit their own event (U26).

### 4.3 Threading by UID

RSVP replies usually come from calendar software that writes to the organizer's address without the
thread token and often without `In-Reply-To`, so they would start new threads. [Threading § 3.1](threading.md#31-order-fr-thr-1)
gains one step after `References`:

> 3b. **Calendar UID.** If the message carries a **verified** calendar object, look up
> `db.thread_of_calendar_uid(uid_hash)`; on a hit, join that thread (`via: CalendarUid`).

```sql
-- ThreadLookup::thread_of_calendar_uid
SELECT thread_seq FROM messages
WHERE calendar_uid_hash = ?1 AND status NOT IN ('hidden','throttled')
  AND (direction = 'outbound' OR json_extract(calendar_json, '$.verified') = 1)
ORDER BY rowid DESC LIMIT 1;
```

- Only a verified object joins by `UID`, and only threads that hold an outbound calendar message or a
  verified inbound one with that `UID`. A stranger who learned a `UID` (it is in every copy of the
  invitation) and sends an object with it is not verified, so it never joins by `UID`; it threads by
  headers or starts a new thread (U29).
- The `join_unverified` rule for header joins applies: a sender who is not a participant of the thread
  joins with `thread_join_unverified` in `trust.flags` ([C9](../edge-cases.md)). A verified `REPLY` from an
  attendee the identity invited is a participant, because the invitation's recipients are.
- The subject is still never consulted, and a thread token or a header match still decides first (U28).

`FR-THR-1` becomes: token; `In-Reply-To` or `References`; the `UID` of a verified calendar object matching
a calendar message of the mailbox; otherwise a new thread. The subject alone never joins a thread.

### 4.4 The `calendar.received` event

A mailbox event, so it carries the identity's `sequence` and follows the outbox, `event_index`, fan-out,
event feed and replay of [Webhooks](webhooks.md) like every `message.*` event.

```json
{
  "id": "evt_01JA9B2C4F5GW7X2M5N6P8R0T1", "type": "calendar.received", "api_version": "2026-10-01",
  "occurred_at": "2026-10-17T08:02:15.120Z", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…",
  "sequence": 1907,
  "data": {
    "message_id": "msg_01JA9B2C3D4EW7X2M5N6P8R0T0", "thread_id": "thr_01JA9B2C3D4EW7X2M5N6P8R0T9",
    "object_status": "parsed", "method": "request",
    "uid": "040000008200E00074C5B7101A82E00800000000F0A1C2D3E4B5DC0100000000000000001000000",
    "sequence": 1, "recurrence_id": null,
    "verified": true, "verified_by": "organizer", "reason": null,
    "organized_here": false
  }
}
```

`data` holds no summary, description, location or address. `recurrence_id` is the primary's time value
(or `null`); `released: true` or `reprocessed: true` is added when the event comes from a release or a
re-parse.

**When it is appended.** In the transaction that first makes the message visible with status `received`:

| Transition | Event |
|---|---|
| Ingest stores the message `received` | Appended (step 16 above) |
| Ingest stores it `quarantined` (any reason) | Not appended |
| Ingest stores it `screening` ([Screening](screening.md)) | Not appended; the transaction that moves it to `received` appends it |
| A quarantined message is released ([Inbound › Read path and release](inbound.md#read-path-and-release), step 5) | Appended with `released: true`; `verified` and `reason` are those computed at ingest: a release makes mail visible, it does not make it authentic |
| `hidden` or `throttled` | Never |
| A re-parse ([Inbound › Re-parsing](inbound.md#re-parsing-j3)) of a `received` message whose new `calendar_json` differs from the stored one (compared as SHA-256 of the JSON) | Appended with `reprocessed: true`; nothing when the object is unchanged |
| The message is in the system identity's mailbox | Never ([A15](../edge-cases.md)) |

**Why held mail emits nothing until it is visible** (U24). The two choices were to emit `verified: false`
at once or to withhold the event. Withholding wins: a quarantined message is visible only to keys with
`quarantine:review` ([FR-IN-5](../prd.md)), while an endpoint subscribed to `calendar.received` needs only
`messages:read`, so an immediate event would reveal that held mail exists and carry a sender-chosen `UID`
past the quarantine; the consumer could not act on it anyway, because it cannot read the message; and
`verification.received` already follows the same rule ([Inbound › Verification codes](inbound.md#verification-codes-and-unsolicited-otp-e5)).
Mail that is visible but unverified does get the event, with `verified: false` and its reason.

**Who may subscribe** ([J29](../edge-cases.md)). `data.uid` is text the sender chose, so
`calendar.received` is a mail event: creating, re-pointing or replaying an endpoint whose `event_types`
contain it (or `["*"]`) needs `messages:read`, as for `message.*` and `verification.received`
([Webhooks › Who may subscribe to mail events](webhooks.md#who-may-subscribe-to-mail-events-j29)). It is
not handed to the Notifier ([Webhooks › Handing new mail to the Notifier](webhooks.md#handing-new-mail-to-the-notifier)):
the invitation's own `message.received` already counts as new mail.

### 4.5 Reading

The Message object gains `calendar` (the object of [§2.9](#29-the-calendar-object), or `null`) wherever a
Message is returned: `GET …/messages/{message_id}`, thread reads, lists, search hits that embed messages,
`wait`, MCP and the replay of a send's response. Its visibility is the message's: no key reaches the object
of a message it cannot read, and its raw bytes are the attachment `attachment_id` under the rules of
[REST API › Attachments](../../reference/api.md#get-v1identitiesidentity_idmessagesmessage_idattachmentsattachment_id--attachmentsread).
`wait` needs no change: an agent waiting for an RSVP waits with `kind=reply` and the invitation's
`thread_id`, and the verified `REPLY` joins that thread by `UID` ([§4.3](#43-threading-by-uid)).

## 5. Outbound: sending iCalendar

Edge rows: U30–U42.

### 5.1 The `calendar` field

`POST /v1/identities/{identity_id}/messages` (send, with or without `thread_id`) and
`POST …/messages/{message_id}/reply` accept:

```json
{
  "to": [ { "address": "jo@example.net", "name": "Jo Rivera" } ],
  "subject": "Vehicle handover: Friday 29 March, 09:00",
  "text": "Your handover for booking BK-42 is on Friday 29 March at 09:00 at our Leeds branch.",
  "kind": "transactional",
  "calendar": {
    "ics": "BEGIN:VCALENDAR\r\nPRODID:-//Kairos Project//Agent Calendar//EN\r\nVERSION:2.0\r\nCALSCALE:GREGORIAN\r\nMETHOD:REQUEST\r\nX-WR-TIMEZONE:Europe/London\r\nBEGIN:VEVENT\r\nUID:handover-BK-42@bookings.acme.example.com\r\nDTSTAMP:20270320T120000Z\r\n…\r\nEND:VCALENDAR\r\n",
    "attach_copy": true
  }
}
```

| Field | Rule |
|---|---|
| `calendar.ics` | The iCalendar object, as text, at most 262,144 bytes of UTF-8 (`413 calendar_too_large` beyond it). Required when `calendar` is present |
| `calendar.attach_copy` | Also attach the same bytes as `invite.ics` ([§5.5](#55-mime-composition)). Default `true` for organizer methods (`request`, `add`, `cancel`, `publish`, `declinecounter`) and `false` for attendee methods (`reply`, `counter`, `refresh`) |

`calendar` is refused on `reply-all` and `forward` (`400 invalid_request`, `details.errors[0].path =
"calendar"`): an invitation names its recipients itself, and reply-all would add recipients the object
does not list. A draft carries the same field in its body and sends it through this same pipeline
([Drafts](drafts.md); the draft send uses idempotency key `draft:{draft_id}:{revision}`).

### 5.2 Validation, in pipeline order

Two steps are added to [Outbound › Policy pipeline](outbound.md#policy-pipeline); the existing steps keep
their numbers and their errors, so a paused identity still gets `409 identity_paused` before any calendar
check.

**Step 9a, "Calendar payload"** (after steps 9 and 10), only when `calendar` is present, in this order:

1. `kind` must be `transactional`: an invitation is never marketing mail (no `List-Unsubscribe`, no consent
   record, and step 15's marketing transport rule never applies), and it is not an automatic reply
   (`400 invalid_request`, `details.errors[0].path = "kind"`).
2. The operation must be `send` or `reply` (above).
3. `ics` at most 262,144 bytes: `413 calendar_too_large`, `details.limit`.
4. Read it with `core::ical` under the bounds of [§2.2](#22-the-bounded-reader) (the 1 MiB part bound is
   replaced by the 256 KiB above). Any reading failure is `400 calendar_invalid` with `details.reason =
   "parse_error"` and `details.line`, the 1-based content line where it failed.
5. Structure, each failure `400 calendar_invalid` with the `details.reason` shown (and `details.property`
   where it names one):

   | Rule | `details.reason` |
   |---|---|
   | One `VCALENDAR`, with `VERSION:2.0` and a `PRODID` ("An iCalendar object MUST include the PRODID and VERSION calendar properties", RFC 5545 §3.6) | `version_missing`, `prodid_missing` |
   | Exactly one `METHOD` (RFC 5545 §3.6: it must not occur more than once), one of the eight methods (iMIP needs one; `none` cannot be sent) | `method_missing`, `multiple_methods`, `method_unknown` |
   | Components: `VEVENT`s and `VTIMEZONE`s only | `component_not_supported` |
   | At least one `VEVENT`; every `VEVENT` has the same `UID`, of at most 1,024 bytes (RFC 5546 §3.2.2) | `uid_missing`, `uid_mismatch`, `uid_too_long` |
   | Every `VEVENT` has a UTC `DTSTAMP` (RFC 5546 tables; RFC 5545 §3.8.7.2) | `dtstamp_missing` |
   | `SEQUENCE`, when present, is an integer from 0 to 2,147,483,647 | `sequence_invalid` |
   | Every `VEVENT` has exactly one `ORGANIZER` with a `mailto:` address, the same in every component | `organizer_missing`, `organizer_inconsistent` |
   | `publish`: no `ATTENDEE` (RFC 5546 §3.2.1) | `attendee_not_allowed` |
   | `request`: at least one `ATTENDEE`, a `DTSTART` and a `SUMMARY` (which may be empty) in each component (RFC 5546 §3.2.2) | `attendee_missing`, `dtstart_missing`, `summary_missing` |
   | `add`: `SEQUENCE` greater than 0 (§3.2.4) | `sequence_invalid` |
   | `cancel`: `SEQUENCE` present (§3.2.5) | `sequence_invalid` |
   | `reply`, `refresh`: exactly one `ATTENDEE` (§3.2.3, §3.2.6); `counter`: at least one (§3.2.7) | `attendee_count` |
   | No `VALARM` in `cancel`, `refresh`, `declinecounter` or `reply` (RFC 5546 tables: 0 for these methods) | `property_not_allowed` (`details.property = "VALARM"`) |
   | `RRULE` valid ([§2.6](#26-recurrence)), at most one per component | `rrule_invalid` |
   | Every `TZID` used is defined by a `VTIMEZONE` of the object, or resolves by name ([§5.3](#53-normalisation)) | `unknown_tzid` (`details.tzid`) |

6. Normalise ([§5.3](#53-normalisation)). The normalised bytes, not the received ones, are what the size check
   of step 17, the stored copy and the transport use.

**Step 13a, "Calendar parties"** (after step 13 has resolved the `From` address, including
`select_reply_from` for a reply or a threaded send and the `retiring` rule of [G7](../edge-cases.md)):

1. **Organizer methods** (`publish`, `request`, `add`, `cancel`, `declinecounter`): the `ORGANIZER` address
   must equal the resolved `From` address, case-insensitively: `400 calendar_organizer_mismatch` with
   `details.organizer` and `details.from_address`. A recipient's calendar software checks the organizer
   against the message, so an invitation whose organizer is not the sender would fail at the recipient,
   and it would let an identity issue invitations in someone else's name. The error names the `From` that
   would be used, so a caller can correct the object: on a fallback-pinned thread that is the identity's
   platform address ([Threading § 5.1](threading.md#51-fallback-pinned-threads)) (U30). An identity that is
   only an attendee cannot delegate by sending a `REQUEST` in v1; it answers with a `REPLY` instead.
2. **Attendee methods** (`reply`, `refresh`, `counter`): the replying `ATTENDEE` (the single one, or for
   `counter` one of them) must equal the resolved `From` address: `400 calendar_attendee_mismatch` with
   `details.attendee` and `details.from_address`.
3. **Recipients**, after step 6 has validated and de-duplicated them: for organizer methods other than
   `publish`, every `to`, `cc` and `bcc` recipient must be an `ATTENDEE` of some component (a `cancel`
   names the attendees it removes, RFC 5546 §3.2.5); for attendee methods there must be exactly one
   recipient, and it must be the `ORGANIZER`; `publish` may go to anyone. Otherwise
   `400 calendar_recipient_mismatch` with `details.address`. Not every attendee has to be a recipient: an
   attendee may be the organizer itself, a room, or invited another way.

**Step 13b, "Calendar sequence"**: [§5.9](#59-the-stale-sequence-guard).

Step 6's address rules, step 7's recipient count, step 12's content rule (at least one of `text` and
`html`: RFC 6047 §2.4 says scheduling messages should carry a human-readable alternative), step 16's
recipient filters, step 17's size limit and step 18's allowance and caps apply unchanged.

### 5.3 Normalisation

`core::calendar::normalise_outbound(ics, ctx) -> (Vec<u8>, OutboundCalendarReport)` is pure and
deterministic, so the same request always produces the same bytes and the same idempotency fingerprint
outcome.

1. **Re-serialise** the parsed object with `core::ical::write`: properties and parameters in their
   received order and case of values, names in upper case, text re-escaped per RFC 5545 §3.3.11, CRLF line
   ends, lines folded at 75 octets without splitting a UTF-8 sequence (RFC 5545 §3.1). Nothing the reader
   did not understand as one of its properties survives as raw bytes, so a payload cannot smuggle a content
   line the reader would not have accepted (U35).
2. **Remove other systems' extensions**: every `X-` property and every `X-` parameter, except
   `X-WR-TIMEZONE` at the calendar level (Kairos writes it, and it names a time zone and nothing else). The
   removed names (at most 20, de-duplicated) are returned in `calendar.outbound.removed`. An extension
   property is another system's internal data (an internal booking reference, a CRM record ID), which an
   invitation would otherwise copy into the recipient's calendar; removing it, rather than refusing it,
   lets a caller send the object its calendar library produced and see what was taken out.
3. **Complete time zones**: for each `TZID` used without a `VTIMEZONE`, when it resolves by name (an IANA
   name, a Windows name through the CLDR table, or a `/`-prefixed IANA name, [§2.5](#25-times-and-time-zones)),
   `core::ical::vtimezone::generate` writes a `VTIMEZONE` with that `TZID` from the tzdb rules in
   `chrono-tz`: it finds the transitions of the years the object covers (from the year before the earliest
   `DTSTART` to the year of the latest end or `UNTIL`; to 10 years after the first start for a rule with
   `COUNT` or no end), and writes one `STANDARD` and one `DAYLIGHT` observance with a yearly `RRULE`
   (`BYMONTH` and an ordinal `BYDAY`) when the last two covered years' transitions follow such a pattern at
   the same local time, otherwise one observance per transition with its own `DTSTART`; a zone without
   daylight saving gets one `STANDARD` with equal offsets. Each generated definition is at most 8 KiB, and at
   most 10 are generated. Their `TZID`s are returned in `calendar.outbound.added_vtimezones`. RFC 5546
   requires a `VTIMEZONE` for every time zone a date refers to (the `VTIMEZONE` row of its §3.2 tables, read
   2026-10-17), and Kairos's `emitITipCalendar` writes `TZID=Europe/London` without one
   (`packages/kairos/src/sync.ts`, read 2026-10-17), so without this step Kairos's invitations would not
   conform. A `TZID` that neither resolves by name nor has a definition was refused in step 9a (U34).
4. Nothing else changes: `UID`, `SEQUENCE`, `DTSTAMP`, times, parties, `VALARM`s allowed by the method,
   `ATTACH`, `DESCRIPTION` and the rest are the caller's content and are sent as given.

### 5.4 Recipients and the known-recipient rule

Invitation recipients go through [Outbound › Recipient filters](outbound.md#recipient-filters) like any
recipient: suppressions, send-block, `send_allowlist_only` and `require_known_recipient`. An attendee the
identity has never written to is `suppressed` with `policy: unknown_recipient` and the send is still
accepted (U39); a dry run reports it. The integrator gives first contact explicitly, with a send-allow
entry or by turning the setting off for a backend identity ([E2](../edge-cases.md)). One rule is added for
the reply route only (rule 5; rule 4 is the approved-draft rule of [Drafts](drafts.md)) ([§6](#6-the-calendar-reply-route)):

> 5. for `calendar_reply` only, it is the `ORGANIZER` of the verified invitation answered: the
>    authenticated sender, or an address of the same organisational domain named by `SENT-BY`
>    ([§3.2](#32-rules-by-method)).

### 5.5 MIME composition

`core::compose` builds a calendar message as RFC 6047 §2.4 and the CalConnect iMIP guide describe it: the
human-readable text, and the calendar part with `method` and `charset` parameters, as alternatives
([iMIP best practices](https://devguide.calconnect.org/imip/best-practices/), read 2026-10-17):

```text
multipart/mixed                                  only when there are attachments or attach_copy is true
├── multipart/alternative
│   ├── text/plain; charset=UTF-8                the request's text (derived from html when missing),
│   │                                            signature and AI disclosure, as for every send
│   ├── text/html; charset=UTF-8                 only when the request has html
│   └── text/calendar; method=REQUEST; charset=UTF-8; component=VEVENT
│                                                the normalised object (last, the richest alternative)
├── application/ics; name="invite.ics"           attach_copy: the same bytes,
│   Content-Disposition: attachment; filename="invite.ics"
└── the request's attachments
```

- The `method` parameter is the object's `METHOD`, upper case; it must match (RFC 6047 §2.4), and it does
  by construction. `component=VEVENT` is the optional parameter of §2.4. The calendar part has no
  `Content-Disposition`, as an inline iMIP body part.
- Transfer encoding: `7bit` when every byte is ASCII (the writer's lines are at most 75 octets), otherwise
  `base64`; RFC 6047 §2.5 requires a transfer encoding for non-ASCII content unless the transport is
  8-bit clean, and base64 never alters the CRLF line ends that quoted-printable soft breaks could confuse
  for a reader. `mail-builder` writes the parts with the explicit boundaries and encodings already required
  by [Outbound › Composition](outbound.md#composition) (its part API is checked at build time, as for every
  other part).
- `invite.ics` is `application/ics` with no `method` parameter, so a client that ignores inline calendar
  parts can still offer to import the file, while no client treats the copy as a second iMIP body.
  `application/ics` is what Google and others use for that copy; RFC 6047 does not define it (read
  2026-10-17).
- The size limit of step 17 counts both copies. The 262,144-byte payload encodes to at most about
  360 KiB per copy, so a calendar can never alone exceed the 5 MiB transport limit.

### 5.6 Transports

| Transport | How a calendar message is sent |
|---|---|
| `cloudflare` | The structured `send()` cannot carry it: its `EmailMessageBuilder` has `text`, `html` and `attachments` (each with a `type` and a `disposition` of `attachment` or `inline`) and no way to add a third alternative with a `method` parameter ([Email Sending Workers API](https://developers.cloudflare.com/email-service/api/send-emails/workers-api/), last updated 2026-10-15, read 2026-10-17). A calendar message is therefore sent with the legacy `EmailMessage` form of `send()`, the composed MIME as raw RFC 5322 bytes, which the same page says "remains supported for backward compatibility" for an existing raw message, and which is already the S1 fallback for any field the structured form lacks ([Design › Spikes](index.md#spikes)). The page shows `EmailMessage` with one sender, one recipient and the raw bytes, and does not say whether it takes several recipients or arbitrary (unverified) ones. Spike S1 gains a case: a raw calendar send to two arbitrary recipients, recording whether it is accepted, whether one `EmailMessage` can carry several envelope recipients, whether the `text/calendar` part and its `method` parameter arrive unchanged, and whether Cloudflare keeps the composed `Message-ID`. Until S1 records that a raw send to an arbitrary recipient works, a calendar send whose transport is `cloudflare` is refused at step 15 with `422 transport_unavailable` and `details.reason = "calendar_needs_raw_mime"`. If S1 shows one recipient per `EmailMessage`, a calendar message on this transport may have one recipient (`400 too_many_recipients`, `details.max = 1`, `details.reason = "calendar_single_recipient"`), because one transport call per message is what the claim and `uncertain` rules rely on (U40) |
| `ses` | `SendEmail` with raw content, the stored `.eml` ([Outbound › Amazon SES](outbound.md#amazon-ses)); nothing changes |
| `smtp` | The customer's relay, raw ([Outbound › SMTP relay](outbound.md#smtp-relay)); nothing changes |
| `simulator`, `loopback` | As for any send. Loopback delivers the composed MIME to the recipient identity, whose ingest reads the object with `verdict: pass`, so a test tenant can exercise invitation, verification and reply end to end (U54) |

**Never through domain fallback** (U31). The `ORGANIZER` (or replying `ATTENDEE`) is bound to the `From`
address. Fallback replaces `From` with the identity's platform address at transport time
([Outbound › From address and fallback](outbound.md#from-address-and-fallback)), which would break that
binding after the object was checked. In `BeginTransport`, a message with `calendar_json` takes the
"fallback disabled" row wherever another message would fall back (a `failing` or `suspended` domain, or a
Cloudflare domain with `delivery_events: manual`): status `failed`, reason `domain_failing_no_fallback`,
nothing sent, the `sends` hold settled with nothing consumed. A held domain (`pending`, `verifying`,
sending-paused) holds a calendar message like any other.

**Message-ID.** On the raw Cloudflare path there is no hidden journal copy (strategy B of
[Outbound › Message-ID](outbound.md#message-id-of-outbound-mail-spike-s7)), because the journal address
would be a second envelope recipient. The composed `Message-ID` (`<{message ulid}@{from domain}>`, as
loopback writes it) is stored as `rfc_message_id`; if S1 shows that Cloudflare replaces it, replies to that
message thread by token and by `UID` only.

### 5.7 `kind`, `Auto-Submitted` and loops

A calendar message is `kind: transactional`: no `Auto-Submitted` header, no `List-Unsubscribe`, and the
automatic-exchange cap of [D6](../edge-cases.md) does not count it. The loop guard of [N13](../edge-cases.md)
applies to it as to every send (`409 loop_detected` at hop 10). iTIP itself cannot loop through this
service: the reply route answers only `request`, never a `reply`, and the service never answers anything on
its own.

### 5.8 Threading of outbound calendar mail

Outbound calendar mail threads as any send ([Threading § 4](threading.md#4-outbound-threading)): a send
without `thread_id` starts a thread, a send with `thread_id` or a reply continues one. Kairos sends an
update or a cancellation for a booking in the thread of its first invitation by passing `thread_id`. The
stored outbound row gets `calendar_json` (the object as sent, `verified: null`) and `calendar_uid_hash`,
so the `UID` join of [§4.3](#43-threading-by-uid) files the attendees' replies into that thread.

### 5.9 The stale-sequence guard

Pylota Mail does not order a calendar's versions; the consumer does (for Pylota, Kairos). It does refuse
one thing it can see, an identity contradicting its own earlier send (U37): for organizer methods, a send
whose primary `SEQUENCE` is lower than the highest `SEQUENCE` this identity has already sent with the same
`UID` in an organizer method gets `409 calendar_sequence_stale` with `details.sequence`,
`details.latest_sequence` and `details.latest_message_id`:

```sql
SELECT id, json_extract(calendar_json, '$.sequence') AS seq FROM messages
WHERE calendar_uid_hash = ?1 AND direction = 'outbound'
  AND status NOT IN ('canceled','rejected','failed','suppressed')
  AND json_extract(calendar_json, '$.method') IN ('request','add','cancel','publish','declinecounter')
ORDER BY seq DESC, rowid DESC LIMIT 1;
```

An equal `SEQUENCE` is allowed (sending the same version to a new attendee, or re-sending after a
`failed`). The query runs at step 13b and again inside the accept transaction of
[Outbound › Reservation](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1), step 7, so two sends that
race through the awaits of submit cannot both pass: the second commit finds the first and is refused,
with its sent copy deleted and its reservations released, like any refusal after the copy was written.

### 5.10 Response and dry run

The `202` Message object carries `calendar` (the object as sent) with
`calendar.outbound = { "attach_copy", "added_vtimezones", "removed" }`. A dry run runs steps 1–17
including 9a, 13a and 13b and returns, besides `would_send` and `recipients`, `calendar: { "method",
"uid", "sequence", "organizer", "added_vtimezones", "removed" }`.

### 5.11 Forwarding an invitation

`forward` re-attaches the original's attachments. A calendar attachment is re-attached as
`application/ics` with filename `invite.ics` and no `method` parameter, never as an iMIP body part, so a
forward never acts as an invitation at the recipient (U42). RFC 5546 §3.2.2.6 lets an attendee forward a
`REQUEST` unchanged; an agent that wants a new person invited asks the organizer, or its consumer sends a
new `REQUEST` as organizer.

### 5.12 What recipients' clients accept

Read on 2026-10-17:

- The CalConnect iMIP guide recommends `text/plain` and `text/calendar` in `multipart/alternative`, the
  `method` parameter matching `METHOD` ("A missing method parameter means many clients won't process the
  calendar part"), UTF-8, the organizer's address as both the SMTP sender and `ORGANIZER`, the recipient's
  address as an `ATTENDEE`, and a `SEQUENCE` increased on every update, because clients silently ignore an
  update without one ([iMIP best practices](https://devguide.calconnect.org/imip/best-practices/)).
  [§5.2](#52-validation-in-pipeline-order) and [§5.5](#55-mime-composition) enforce the first five; the
  last is the consumer's (Kairos increments its revision).
- Google Calendar can show invitations "only if they come from a sender you know" (people in the user's
  domain, contacts, or people the user has interacted with), and otherwise only once accepted; the default
  shows invitations from everyone ([Google Workspace Updates, 20 July 2022](https://workspaceupdates.googleblog.com/2022/07/invitations-from-known-senders-only-google-calendar.html)).
  A first invitation from an operator to a renter may therefore wait for the renter's acceptance.
- Microsoft documents its conversion between iCalendar and Exchange appointments in
  [MS-OXCICAL](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcical/a685a040-5b69-4c84-b084-795113fb4012)
  (revision 15.0, 2025-05-20); its rules for each method were not read this run.

Not verified this run, and decided by the live tests of [§18](#18-tests) instead: whether Gmail,
Outlook.com, Microsoft 365 and Apple Calendar show exactly one invitation when the `application/ics` copy is
attached (if one shows two, the default of `attach_copy` becomes `false` for that method and this page
records it); whether each applies a `REPLY` that carries only the properties of [§6.3](#63-the-reply-that-is-built);
whether Outlook accepts a generated `VTIMEZONE` with an `RRULE`; and whether any of them treats
`Auto-Submitted` on a calendar message differently (the service sets none).

## 6. The calendar-reply route

Edge rows: U43–U50.

### 6.1 Request and response

`POST /v1/identities/{identity_id}/messages/{message_id}/calendar-reply`, permission `messages:send`, every
key level, rate bucket `RL_SEND` (per identity, as every send), `Idempotency-Key` required (the send rules:
`400 idempotency_key_required`, `400 invalid_idempotency_key`, 30 days, scoped per identity), `?dry_run=true`
supported as on sends.

```json
{ "partstat": "accepted", "comment": "Our driver will bring the car at 08:45.", "recurrence_id": null }
```

| Field | Rule |
|---|---|
| `partstat` | `accepted`, `declined` or `tentative`. Required |
| `comment` | Optional, at most 1,000 characters; LF allowed (written as `\n`), CR removed, other control characters `400 invalid_request` (`details.errors[0].path = "comment"`) |
| `recurrence_id` | Optional. Answers one instance: an RFC 3339 instant when the invitation's `DTSTART` is a date-time, or `YYYY-MM-DD` when it is a date (else `400 invalid_request`, path `recurrence_id`). Not checked against the `RRULE`, which is never expanded; the organizer decides what an answer to an instance means |

`202` with the Message object (`direction: "outbound"`, `status: "queued"`, `operation: "reply"`, the
`calendar` object of the `REPLY`) and `"deduplicated": false`, or the stored response with
`"deduplicated": true` and `Idempotent-Replayed: true`, as for every send.

### 6.2 Rules in order

Inside `IdentityMailbox.submit`, after the idempotency lookup and the in-flight check (so a replay returns
the original response whatever has changed), the policy pipeline runs steps 1–4 as for any send (identity
deleted, tenant suspended, identity paused, tenant sending paused, no accountable human). Step 5 (target) is
then, for this operation:

1. The message exists in this mailbox and is visible to the key, as for a reply: `404 message_not_found`
   for a missing, `hidden` or `throttled` one, or a `quarantined` one without `quarantine:review`.
2. It is inbound, has a calendar object with `object_status: "parsed"` and `method: "request"`: otherwise
   `409 calendar_not_invitation`, with `details.method` and `details.object_status` (`null` when there is
   no object).
3. It is not `quarantined`: `409 calendar_not_verified` with `details.reason = "message_quarantined"`;
   release it first (a release by a person, [FR-CON-6](../prd.md)).
4. Its object is verified: `409 calendar_not_verified` with `details.reason` = the object's `reason`.
5. One of its `ATTENDEE`s is the identity (`is_self`; when several are, the one equal to `delivered_to`,
   else the first): `409 calendar_not_attendee`.
6. `recurrence_id`, when given, is not one of the invitation's `EXDATE` values: `400 invalid_request` with
   `details.reason = "instance_excluded"`.
7. Not cancelled: the primary (or the override for `recurrence_id`) has `STATUS:CANCELLED`, or the mailbox
   holds a later verified `CANCEL` with the same `UID` whose `SEQUENCE` is at least the invitation's and
   whose scope covers the answer (no `RECURRENCE-ID`, the same `RECURRENCE-ID`, or a `RANGE=THISANDFUTURE`
   at or before it): `409 calendar_cancelled` with `details.cancel_message_id` when a `CANCEL` decided it.
8. Not superseded: the mailbox holds a verified `REQUEST` with the same `UID` and scope and a higher
   `SEQUENCE`, or the same `SEQUENCE` and a later `DTSTAMP` (RFC 5546 §2.1.5): `409 calendar_superseded`
   with `details.latest_message_id` and `details.latest_sequence`. Answer the latest version instead.
9. Not past: `ends_at` of the invitation, or for `recurrence_id` that instance's start plus the primary's
   duration, is before now: `409 calendar_event_past` with `details.ended_at`. An open-ended recurrence
   (`ends_at: null`) is never past.
10. The attendee's address can be the `From`: it is an `active` address of the identity, or `retiring` on
    this thread ([G7](../edge-cases.md)), and the thread is not fallback-pinned to the platform address
    unless that is the attendee address: otherwise `409 calendar_attendee_address_unavailable` with
    `details.address` and `details.reason` (`retired`, `pending`, `fallback_pinned`). The organizer checks
    that a reply comes from the attendee it names, so the reply must not come from another address.

Steps 7 and 8 read only this mailbox, in the same transaction:

```sql
SELECT id, calendar_json FROM messages
WHERE calendar_uid_hash = ?1 AND direction = 'inbound' AND status = 'received' AND rowid <> ?2
  AND json_extract(calendar_json, '$.verified') = 1
  AND json_extract(calendar_json, '$.method') IN ('request','cancel');
```

They are not a calendar: they compare the invitation with the other verified messages the identity
received about the same `UID`, and refuse an answer that the organizer would ignore anyway. The consumer
keeps the authoritative state.

The remaining policy steps then run as for a reply, with `to` = the `ORGANIZER` (never `From` or
`Reply-To`: responses go to the organizer, RFC 6047 §2 and RFC 5546 §3.2.2.5, read 2026-10-17),
`from_address` = the attendee address, `kind: transactional`, the subject and text of
[§6.3](#63-the-reply-that-is-built), and the built `REPLY` as `calendar` (attendee method:
[§5.2](#52-validation-in-pipeline-order) step 13a holds by construction). The organizer is a known
recipient by rule 5 of [§5.4](#54-recipients-and-the-known-recipient-rule); a suppressed or send-blocked
organizer makes the delivery `suppressed`, and the call still answers `202`.

### 6.3 The REPLY that is built

`core::calendar::build_reply(invitation, attendee, partstat, comment, recurrence_id, dtstamp)`:

```text
BEGIN:VCALENDAR
PRODID:-//Pylota Mail//Calendar reply 1//EN
VERSION:2.0
METHOD:REPLY
BEGIN:VEVENT
UID:<the invitation's UID>
DTSTAMP:<dtstamp, UTC>
SEQUENCE:<the invitation's SEQUENCE; omitted when it is 0>
RECURRENCE-ID[;TZID=…|;VALUE=DATE]:<recurrence_id, in the primary DTSTART's form; only when given>
ORGANIZER[;CN=<the invitation's CN>]:mailto:<organizer address>
ATTENDEE;PARTSTAT=ACCEPTED[;CN=<the identity's display name>]:mailto:<attendee address>
COMMENT:<comment, escaped; only when given>
END:VEVENT
[the invitation's VTIMEZONE for that TZID, only when RECURRENCE-ID needs it]
END:VCALENDAR
```

These are the properties RFC 5546 §3.2.3 requires for a `VEVENT` `REPLY` (read 2026-10-17): `METHOD`, one
`ATTENDEE` with the replier's address, `DTSTAMP`, `ORGANIZER` and the invitation's `UID`, plus `SEQUENCE`,
which must equal the invitation's when non-zero, and the optional `RECURRENCE-ID` and `COMMENT`. Optional
properties that a reply carries must not differ from the request's (§3.2.3), and none is copied.

- **`DTSTAMP`** is the submit time, in whole seconds, and strictly later than every earlier `REPLY` this
  identity sent with the same `UID`: `max(now, latest + 1 s)`, read from the mailbox's outbound rows. When
  UID, `RECURRENCE-ID` and `SEQUENCE` are equal the organizer applies the reply with the latest `DTSTAMP`
  (RFC 5546 §2.1.5), so an "accept" followed within the same second by a "decline" is still applied in
  order (U49).
- **Subject**: `Accepted: `, `Declined: ` or `Tentative: ` followed by `normalise_subject` of the
  invitation's subject ([Threading § 7](threading.md#7-subject-normalisation-c8)), cut to 998 characters.
- **Text**: `{display name} ({attendee address}) has accepted the invitation.` (`declined`, `tentatively
  accepted`), then a blank line and the comment when given; the identity's signature and the AI disclosure
  are appended as on any send. No HTML part.
- **Thread**: the invitation's thread, with `In-Reply-To` and `References` built from the invitation
  ([Threading § 4](threading.md#4-outbound-threading)), and the thread lock of
  [C4](../edge-cases.md).

### 6.4 Idempotency

The fingerprint is `send_fingerprint(CalendarReply, "{identity_id}/{message_id}", body)` (the operation is
added to `SendOp`). The stored response holds the composed `DTSTAMP`, so a replay sends nothing and returns
the same `REPLY`. A second answer needs a new key; it builds a new `REPLY` with a later `DTSTAMP`, and both
are sent. The stored message's `operation` column is `reply`, so existing readers of that column treat it
as one.

## 7. Concurrency and idempotency

| Race | What decides it |
|---|---|
| Two calendar sends with one `Idempotency-Key` | The send ledger ([G1](../edge-cases.md)): one email; the second gets the replay, `409 idempotency_conflict` or `409 request_in_progress` |
| Two sends with different keys and different `SEQUENCE`s for one `UID` | The stale-sequence guard re-runs in the accept transaction ([§5.9](#59-the-stale-sequence-guard)); the lower one that commits second is refused |
| Two calendar replies to one invitation | Each is a send with its own key; the thread lock serialises them ([C4](../edge-cases.md)); each `DTSTAMP` is later than the previous one ([§6.3](#63-the-reply-that-is-built)) |
| An invitation and its update arrive out of order | Both are stored and evented with their `sequence` and `dtstamp`; the consumer orders them (RFC 5546 §2.1.5). The reply route's checks 7 and 8 read whatever has arrived by then |
| A `CANCEL` arrives while a reply to the `REQUEST` is in submit | The reply's checks run in the mailbox, which serialises requests; a reply accepted before the `CANCEL` was stored is sent, as an attendee's reply crossing a cancellation in the post would be |
| A reply is submitted while the invitation is being erased | The mailbox serialises them; after the erasure the reply gets `404 message_not_found` |
| Redelivery of an inbound invitation | Duplicate by hash: one row, one `calendar.received` ([B14](../edge-cases.md)) |
| The consumer crashes after `ingest` | The retry finds the duplicate and appends nothing ([J2](../edge-cases.md)) |

## 8. Failure handling

| Failure | Behaviour |
|---|---|
| The reader panics on a calendar part | As every parser panic: queue retry, degraded (headers-only) parsing from the third attempt, which skips step 10a, so the message is stored without its object and never dropped ([J8](../edge-cases.md), U10) |
| The reader returns `invalid`, `too_large` or `too_complex` | The message is stored and delivered by the existing rules with that `object_status`; the event is emitted with `verified: false` and the matching reason (U10, U11) |
| D1 fails during step 12's address query | The consumer's facts batch fails as a whole and the message is retried (`retry(30 s)`), as for every step 12 fact |
| A `VTIMEZONE` cannot be evaluated, or a `TZID` is unknown | The affected times have `utc: null` and a warning; nothing else changes (U3) |
| The CLDR table does not know a Windows name | The `TZID` resolves through a `VTIMEZONE` if the object has one, else `unknown` |
| Outbound payload refused | A `4xx` before anything is stored, reserved or locked (every check runs in steps 9a, 13a and 13b, before step 18); a retry with the same key re-evaluates it, as for every refused send |
| Cloudflare raw path unavailable (S1) | `422 transport_unavailable` (`calendar_needs_raw_mime`) at submit; nothing stored. SES and SMTP domains are unaffected (U40) |
| Transport timeout, connection lost, unknown provider answer | `uncertain`, never resent, as for every send ([FR-OUT-2](../prd.md)) |
| The sending domain fails after submit | `failed` (`domain_failing_no_fallback`) at `BeginTransport`, never sent from the platform address (U31) |
| The stale-sequence re-check fails in the accept transaction | The sent copy and the compose marker are deleted, both reservations released, `409 calendar_sequence_stale` returned; nothing was sent ([I9](../edge-cases.md)) |
| The reply route's mailbox reads time out | `503 unavailable` or `504 timeout` before anything is queued; a retry with the same key is safe |

## 9. Security and visibility

- **Permissions.** No new permission. Reading the object is `messages:read`; fetching the raw `.ics` is
  `attachments:read`; sending a calendar message or a calendar reply is `messages:send`, held at every key
  level; subscribing an endpoint to `calendar.received` needs `messages:read` besides `webhooks:manage`
  ([§4.4](#44-the-calendarreceived-event)).
- **Scope.** The route joins the deny-by-default router table with `Scope::IdentityPath`, and
  `it::security::cross_tenant_matrix`: another tenant's identity or message, or another partner's, gets
  `404 identity_not_found` or `404 message_not_found` exactly as a missing one ([NFR-SEC-1](../prd.md)).
  An identity key reaches only its own identity's invitations.
- **Visibility.** The object of a `quarantined` message is returned only with `quarantine:review`; of a
  `hidden` or `throttled` one, never; no event leaves the mailbox until the message is visible
  ([§4.4](#44-the-calendarreceived-event)).
- **Spoofing.** A forged organizer, cancellation or reply is reported as `verified: false` with its reason
  and can never be answered through the reply route ([§3](#3-authenticity)), never joins a thread by `UID`
  ([§4.3](#43-threading-by-uid)), and Pylota's handler never imports it ([§15](#15-pylota-agentic-os)).
  Flooding an identity with invitations (RFC 5546 §6.1.5) is bounded by the inbound volume caps of
  [D13](../edge-cases.md), and a service that never adds invitations to a calendar cannot have its free time
  blocked by them.
- **Untrusted text.** Every string of the object is untrusted ([§2.8](#28-text-fields)); hidden characters
  are removed; triage and search are unchanged (they read the message's text, which already carries the
  invitation's human-readable part).
- **No remote fetch.** `TZURL`, `ATTACH`, `URL` and `ALTREP` values are never fetched ([Security § 8.4](security.md#84-no-remote-fetch)).
- **Outbound leaks.** Extension properties are removed before an invitation leaves ([§5.3](#53-normalisation));
  the identity can issue invitations only as itself ([§5.2](#52-validation-in-pipeline-order) step 13a), and
  only to addresses that pass the recipient filters.
- **Delegation and forwarding** are reported, never acted on ([§3.2](#32-rules-by-method)).

## 10. Privacy

| What | Rule |
|---|---|
| Where calendar data lives | `messages.calendar_json` and `calendar_uid_hash`, and the calendar attachment in R2: the message's own row and objects. No other store, no D1 row |
| Retention | With the message: `retention.message_days`, and the raw-retention rules for the attachment and the `.eml` |
| Message, thread, identity and tenant erasure | The row is deleted (`EraseRows`), the attachment object with it, and the outbox rows of `calendar.received` (their `data.message_id` names the message, [Privacy § 6.2](privacy.md#62-what-eraserows-deletes)). Receipt counts are those of the message (`messages_deleted`, `attachments_deleted`, `r2_objects_deleted`, `events_deleted`) |
| Counterparty erasure | A message also matches when the normalised address is the object's organizer, an organizer `SENT-BY`, or an attendee (or attendee `SENT-BY`, `DELEGATED-FROM` or `DELEGATED-TO`) of any kept component, because the object exposes those addresses in a structured form. `ErasurePlan { Counterparty }` and `ErasurePlan { CounterpartyHash }` add `EXISTS (SELECT 1 FROM json_each(calendar_json, '$.parties') p WHERE p.value = ?)`, where `?` is the first 16 bytes, as lower-case hexadecimal, of `HMAC-SHA256(PM_HASH_KEY, address)` (the counterparty hash the erasure record already keeps, [Privacy § 4.1](privacy.md#41-mailbox-operations-used-by-jobs)), so both targets use one comparison ([Privacy § 6.4](privacy.md#64-counterparty-scope-i1), U51) |
| Legal hold | A held thread keeps its calendar messages, as all its messages ([Privacy § 8](privacy.md#8-legal-holds)) |
| Subject-access export | `messages.json` gains `calendar` for each message ([Privacy § 9.3](privacy.md#93-messagesjson), U52) |
| Logs | Never the `UID`, a summary, a location, a description or an address; the method, `object_status`, `verified`, the reason and the `tz_resolution` values only ([Security § 12](security.md#12-logging-rules), U52) |
| Webhook payloads | `calendar.received` carries the `UID` and IDs only; integrators apply erasure to their copies ([Privacy § 11](privacy.md#11-what-remains-after-deletion)) |

`parties` is an internal field of `calendar_json`, never returned by the API, MCP or exports: the
truncated HMAC of every distinct normalised address of the object (at most 1,000, [§2.2](#22-the-bounded-reader)),
written by the consumer at step 12 with `PM_HASH_KEY`. It makes counterparty matching one `json_each` over a
bounded array, holds no clear address, and covers attendees that the shrink order of
[§4.1](#41-columns-and-caps) removed from the visible list.

## 11. Billing and limits

**Billing.** A calendar send and a calendar reply are sends: the `sends` hold takes one unit per recipient
that is not suppressed, `402 billing_limit` is safe to retry with the same key, and the daily caps apply
([FR-BILL-4](../prd.md), [FR-BILL-5](../prd.md), U53). Reading, verifying and storing inbound invitations
costs nothing and is never refused for a plan (FR-BILL-8). No plan has a calendar allowance.

**Limits** (Pylota Mail choices unless a source is named):

| Limit | Value | Why |
|---|---|---|
| Calendar parts read per message | 10 | More is never seen in practice; later parts stay attachments |
| Decoded calendar part read | 1 MiB | Real invitations are 2–30 KiB; AgentMail imports at most 1 MB; Kairos accepts up to 2,000,000 bytes |
| Content lines; one unfolded line | 20,000; 64 KiB | Linear work, bounded memory |
| Components per object | 500 | Kairos reads up to 2,000 `VEVENT`s |
| Overrides read; kept in the object | 1,000; 100 | Kairos keeps 1,000 exceptions per series |
| Attendees read per component; kept | 1,000; 100 | Kairos and AgentMail both cap events at 100 attendees |
| `VTIMEZONE`s; observances; `RDATE`s per observance | 20; 50; 100 | Real definitions have 1–4 observances |
| `EXDATE`, `RDATE` values per component | 10,000 each | |
| `UID` | 1,024 bytes | RFC 5545 §3.8.4.7 requires at least 255 octets |
| Distinct party addresses per object (`parties`) | 1,000 | Counterparty erasure must see every address the object exposes ([§10](#10-privacy)) |
| `summary`; `location`; `description` excerpt; `comment`; `CN` | 1,000; 1,024; 2,000; 2,000; 256 characters | AgentMail allows 1,000-character titles and 1,024-character locations |
| `RRULE` text; `INTERVAL`; `COUNT`; `BY…` list | 512 bytes; 1–1,000; 1–100,000; 366 values | A valid rule must not be a trap for an expanding consumer |
| `calendar_json` | 262,144 bytes, with the shrink order of [§4.1](#41-columns-and-caps) | The 1,900,000-byte row budget |
| Outbound `calendar.ics` | 262,144 bytes (`413 calendar_too_large`) | Two copies fit inside the 5 MiB transport limit many times |
| Generated `VTIMEZONE`s | 10 per message, 8 KiB each | |
| Calendar-reply `comment` | 1,000 characters | |
| Calendar recipients on the `cloudflare` transport | As `max_recipients`, or 1 if S1 shows that a raw send takes one recipient | [§5.6](#56-transports) |

## 12. Events

| Event | When | `data` |
|---|---|---|
| `calendar.received` | A message with a calendar object first becomes visible as `received` (ingest, release, the end of screening), or a re-parse changed the object of a `received` message ([§4.4](#44-the-calendarreceived-event)) | `message_id`, `thread_id`, `object_status`, `method`, `uid`, `sequence`, `recurrence_id`, `verified`, `verified_by`, `reason`, `organized_here`; `released: true` or `reprocessed: true` when applicable |

Outbound calendar messages emit the existing `message.*` events only; the reply route emits what a reply
emits. Every event is also readable from the event feed with the same `evt_` ID ([Event feed](event-feed.md)).

## 13. Console, MCP and CLI

**Console.** The thread view ([Console › Showing untrusted mail](console.md#showing-untrusted-mail)) shows a
**Calendar** panel above a message that has an object, for every role: the method; a verified badge, or
"Not verified" with the reason in words; the summary and location (escaped text); the start and end in the
workspace's time zone, with the original `TZID` and the resolution; "all day" or "floating" when they apply;
the organizer; the attendee count and the identity's own answer; warnings; and a link to the `.ics` through
the console's attachment route. A quarantined message shows only the method and the verified badge, as it
shows only the text view. There are no answer buttons: answering is an agent's or the integrator's action
through the API, and the console is not a webmail client ([PRD § 4](../prd.md#non-goals-v10)).

**MCP** ([MCP › Tools](mcp.md#4-tools)):

| Tool | What | Permission |
|---|---|---|
| `mail_calendar_reply` (new) | `POST …/messages/{message_id}/calendar-reply`. Arguments: `identity`, `message_id`, `partstat`, `idempotency_key` (required); `comment`, `recurrence_id` | `messages:send` |
| `mail_send`, `mail_reply` | Gain the optional argument `calendar` (`{ ics, attach_copy }`) | `messages:send` |
| `mail_get_message`, `mail_get_thread` | Return `calendar`; MCP keeps at most 20 attendees and 10 overrides per object, with `attendees_truncated` and `overrides_truncated` | `messages:read` |

Annotations of `mail_calendar_reply`: `readOnlyHint: false`, `destructiveHint: false`,
`idempotentHint: true`, `openWorldHint: true` (as `mail_reply`). Its rate bucket is `RL_SEND`.

**CLI** ([CLI reference › Sending](../../reference/cli.md#sending)):

```text
pmail calendar-reply <message-id> --identity <identity> --partstat accepted|declined|tentative
                     [--comment <text>] [--recurrence-id <time-or-date>] [--idempotency-key <key>] [--dry-run]
pmail send … [--calendar-file <file.ics>] [--calendar-copy | --no-calendar-copy]
pmail reply … [--calendar-file <file.ics>] [--calendar-copy | --no-calendar-copy]
```

`pmail messages get` prints the calendar object; `--json` returns it unchanged.

## 14. Observability

**Metrics** ([Observability § 3.2](observability.md#32-catalogue)):

| Metric | Type | Labels | Written by |
|---|---|---|---|
| `calendar_objects_total` | counter | `method`, `object_status` | `pm-inbound`, step 10a |
| `calendar_verification_total` | counter | `method`, `result` (`verified` or the reason) | `pm-inbound`, step 10a |
| `calendar_tz_resolution_total` | counter | `resolution` (`utc`, `iana`, `windows`, `prefixed`, `vtimezone`, `unknown`, `floating`, `date`) | `pm-inbound`, step 10a |
| `calendar_events_total` | counter | `trigger` (`ingest`, `release`, `screening`, `reparse`) | mailbox |
| `calendar_sends_total` | counter | `method`, `result` (`accepted`, or the error code) | `fetch` (send, reply, draft send) |
| `calendar_replies_total` | counter | `partstat`, `result` (`accepted`, or the error code) | `fetch` (calendar-reply) |
| `calendar_vtimezones_added_total` | counter | – | `fetch` |
| `calendar_properties_removed_total` | counter | – | `fetch` (one per removed extension property) |

**Alerts** ([Observability § 5.3](observability.md#53-alert-list)):

| Alert | Class | Condition | Severity | Runbook |
|---|---|---|---|---|
| `calendar_parse_failures` | A | `calendar_objects_total{object_status="invalid"}` is more than 20% of `calendar_objects_total` over 24 hours, with at least 50 objects | ticket | [Parser bug](observability.md#parser-bug) (a reader regression, or a sender's new format; add the case to the corpus) |
| `calendar_raw_refused` | A | `calendar_sends_total{result="transport_unavailable"}` > 0 over 1 hour | ticket | Read the S1 result in [Design › Spikes](index.md#spikes); until the raw path works, calendar mail needs an SES or SMTP domain |

**Log events** ([Observability § 2.2](observability.md#22-event-names)): `calendar_parsed` (info: method,
`object_status`, `verified`, reason, part count, `tz_resolution` values, bytes) and `calendar_send_refused`
(info: method, error code, `details.reason`). Never the `UID`, text or addresses.

## 15. Pylota agentic OS

Pylota reaches Pylota Mail Cloud as a partner ([FR-KEY-4](../prd.md)); each operator is a tenant with four
identities (`bookings`, `inquiry`, `maintenance`, `compliance`) and their identity keys K3; Pylota's servers
hold the partner key K1 and the tenant keys K2 (Pylota's integration spec §4 and §6, read 2026-10-17).
Kairos (`packages/kairos`) is Pylota's calendar for vehicles and appointments; it owns every calendar
fact, including `SEQUENCE` ordering and conflicts, which PostgreSQL arbitrates
(`packages/kairos/SPEC.md`, read 2026-10-17). Pylota Mail is its mail transport and verifier.

**Who uses it, with which keys.**

| Flow | Pylota component | Key and permissions |
|---|---|---|
| Pickup and return appointments: Kairos's `REQUEST` for a handover, its updates and its `CANCEL` | A backend sender in `apps/api` (not an agent), from the operator's `bookings` identity | K3 of `bookings`: `messages:send` |
| Renters' `REPLY` to those invitations | The partner webhook endpoint (`calendar.received`), then Kairos | K1 for the endpoint (`webhooks:manage`, `messages:read`); K2 or K3 `messages:read` and `attachments:read` to fetch the message and the `.ics` |
| Invitations operators receive: a garage's service slot to `maintenance`, an insurer's inspection to `compliance`, a corporate customer's meeting to `bookings` | The same endpoint; Kairos import for verified ones; the operator inbox for all | As above |
| Answering an invitation | The operator coordinator, or the specialist mapped to that identity by `SPECIALIST_MAIL_PURPOSES` (for example `maintenance` and `vendor-ops` for `maintenance`), through Pylota's approval gate | K3 of that identity: `messages:send` |
| The public-webchat coordinator | Never: it reads and writes untrusted conversations, so it gets no calendar tool and never sees a calendar object | – |

**Inbound.** The webhook handler stores `calendar.received` in `mail_webhook_events` like every event and
processes it in the `mail-webhook-event` job:

1. `verified: false` (any reason) or `object_status` other than `parsed`: nothing goes to Kairos. The
   message stays ordinary mail in the operator inbox, which shows "Calendar invitation, not verified:
   {reason in words}".
2. `verified: true`: fetch the message (`GET …/messages/{message_id}`) for its `calendar` object and the
   `.ics` (`GET …/attachments/{attachment_id}`), and hand both to the Kairos iTIP adapter, keyed by
   `(identity_id, uid, recurrence_id)`:
   - `reply` with `organized_here: true`: update that attendee's participation status on the Kairos event
     whose `UID` it is, under Kairos's expected-revision rule; Kairos ignores a reply whose `SEQUENCE` is
     older than its event's (RFC 5546 §2.1.5, its own rule). `organized_here: false`: mail only.
   - `request`, `add`, `cancel` from an external organizer: an untrusted busy-time input for the resource
     the operator maps the sender to, through Kairos's provider boundary (`parseICalendarChanges`, which
     keeps no summaries, parties or notes), or a pending item for the operator; never a trusted Kairos
     event without an operator's decision, because Kairos treats external events as "untrusted busy-time
     inputs, not scheduling authority" (`SPEC.md`, External calendars).
   - `counter`: a pending decision for the operator, through the approval gate.
   - `refresh` with `organized_here: true`: Kairos re-sends its latest `REQUEST` for that `UID` to that
     attendee through Pylota Mail (only to an attendee of the event: RFC 5546 §6.1.6).
   - `publish`, `declinecounter`, `none`: mail only in v1.
3. Instance-only objects (a `CANCEL` of one occurrence has no master component) are mapped from Pylota
   Mail's structured object (`recurrence_id`, `overrides`), because Kairos's `parseTrustedICalendarEvents`
   refuses an exception without its master (`sync.ts`, read 2026-10-17).
4. All-day invitations from Google carry no `X-WR-TIMEZONE`, and Kairos refuses an all-day event without a
   calendar time zone, so the adapter passes the resource's time zone (`calendarTimezone`).

**Outbound.** Kairos already serialises `PUBLISH`, `REQUEST`, `REPLY` and `CANCEL` with
`emitITipCalendar` and validates the result with `ical.js` before it leaves the package. Pylota sends it:

```text
POST /v1/identities/{bookings identity}/messages
Idempotency-Key: pm:v1:kairos-itip:{tenantRef}:{externalRef}:{revision}:{method}

{ "to": ["renter@example.org"], "subject": "Vehicle handover: Friday 29 March, 09:00",
  "text": "…", "thread_id": "{the booking's mail thread, after the first invitation}",
  "calendar": { "ics": "{emitITipCalendar(event, 'REQUEST')}" } }
```

- Kairos's `organizer.email` must be the `bookings` identity's primary address, read from
  `mail_identities.address`, or the send gets `400 calendar_organizer_mismatch` (the error names the
  address Pylota Mail will use).
- The key uses durable IDs only: Kairos's `(tenantRef, externalRef)` and its strictly increasing revision
  (the key rules of integration spec §7.3), so a redrive of the same revision never sends
  twice and a new revision is a new message. Kairos's revision should be the iCalendar `SEQUENCE`, so the
  stale-sequence guard of [§5.9](#59-the-stale-sequence-guard) never refuses a legitimate update.
- Kairos writes `TZID=Europe/London` without a `VTIMEZONE`; Pylota Mail adds the definition
  ([§5.3](#53-normalisation)). It also removes Kairos's `X-` properties except `X-WR-TIMEZONE`.
- The `bookings` identity keeps `require_known_recipient: true`, so the first invitation to a renter is
  `suppressed` (`unknown_recipient`) unless the renter is known. The sender must first add a send-allow
  entry from the booking record (`allowRecipient(operatorId, renterEmail, 'booking_on_file', bookingId)`,
  integration spec §7.7), never from model input.
- Appointments are sent from a domain with the `ses` or `smtp` transport until spike S1 records that the
  raw Cloudflare path works; an operator on the platform domain `pylotamail.com` (a `cloudflare`-transport
  domain) gets `422 transport_unavailable` (`calendar_needs_raw_mime`) meanwhile, and Pylota sends the
  appointment as ordinary mail without the calendar part.

**Answering.** The coordinator or specialist calls Pylota's capability route, which calls
`POST …/calendar-reply` with the identity's K3 and an idempotency key
`pm:v1:calendar-reply:{messageId}:{decisionId}` (the approval row's ID, so a person's decision is sent once).
The tool-risk registry classes it as a send that needs the operator's approval unless the organizer is a
counterparty on file; the agent sees `calendar_not_verified`, `calendar_cancelled`, `calendar_superseded`,
`calendar_event_past` and `calendar_not_attendee` as tool results it can explain to the operator.

**What Pylota stores.** Nothing new in Pylota Mail. In Pylota: `calendar.received` rows in
`mail_webhook_events` (thin payload, nulled when processed), the Kairos events and participation statuses
(Kairos's own tables, forced RLS), the outbound invitations in `mail_outbound_messages` with sender kind
`kairos_itip`, and a link table from `(identity, message_id)` to the Kairos event identity, erased with the
operator and by renter erasure.

**How failures surface.** An invitation refused at submit (`400 calendar_*`, `409 calendar_sequence_stale`)
is a Kairos or adapter bug: logged as an error with the code and paged when it repeats; it is never
retried with a new key. `422 transport_unavailable` pages ops. A `suppressed`, `bounced`, `failed` or
`uncertain` invitation follows integration spec §8.3 ("Not sent", "Not delivered", the uncertain queue);
the booking screen shows that the renter has no invitation. An unverified inbound invitation never reaches
Kairos.

**What Pylota's integration spec must add** (for its owner):

1. `calendar.received` in the partner endpoint's explicit event list (§8.1); K1 already holds
   `messages:read`.
2. The Kairos iTIP adapter and its mapping rules (step 2 above), as a task with tests: verified only;
   `reply` only for `organized_here`; external `request`/`add`/`cancel` as untrusted busy time or an
   operator decision; instance-only objects from Pylota Mail's object; all-day time zone from the resource.
3. The Kairos invitation sender: row in §7.3 (sender `kairos-itip`, identity `bookings`, kind
   `transactional`, key `pm:v1:kairos-itip:{tenantRef}:{externalRef}:{revision}:{method}`), the organizer
   address rule, the send-allow entry from the booking record (§7.7, reason `booking_on_file`), and the
   transport rule above.
4. That Kairos is the owner of `SEQUENCE` ordering and conflicts: its revision is the `SEQUENCE`, it
   applies RFC 5546 §2.1.5 to replies, and it never relies on Pylota Mail to order versions.
5. The `calendar_reply` capability route and its tool-risk registry entry, with the idempotency key above,
   for the operator coordinator and the mapped specialists only; not for the public-webchat coordinator.
6. The operator inbox's "not verified" badge, and the booking screen's invitation status.
7. Erasure: the link table and the Kairos participation statuses in the renter and operator erasure lists.

## 16. Functional requirements

To add to [PRD § 6](../prd.md#6-functional-requirements) as a new subsection, "Calendar invitations":

- **FR-CAL-1** Every inbound message **must** have each of its calendar parts (`text/calendar`,
  `application/ics`, `.ics` files and calendar files inside TNEF; never parts of nested messages, DSNs or
  MDNs) read under size, line, component, override, attendee, time-zone and string bounds, and **must**
  expose one structured, bounded `calendar` object, or `null`. A part that cannot be read **must not**
  stop the message from being stored; its object says why (`invalid`, `too_large`, `too_complex`,
  `unsupported`).
- **FR-CAL-2** Every time in the object **must** be given as a UTC instant where one can be computed, with
  the original `TZID` and how it was resolved: IANA names, Windows names through the CLDR mapping,
  `/`-prefixed globally unique names, and the object's own `VTIMEZONE` definitions. All-day dates,
  floating times and unknown zones **must** be marked as such, never guessed.
- **FR-CAL-3** Recurrence rules **must** be validated and returned as text, never expanded; a master with
  up to 100 overrides **must** be read as one object.
- **FR-CAL-4** Every iTIP method (`PUBLISH`, `REQUEST`, `REPLY`, `ADD`, `CANCEL`, `REFRESH`, `COUNTER`,
  `DECLINECOUNTER`) **must** be classified, as well as objects without a method and with an unknown one.
  `VTODO`, `VJOURNAL` and `VFREEBUSY` **must** be listed as ignored; `VALARM`, `ATTACH` and extension
  properties **must not** appear in the object agents read.
- **FR-CAL-5** Each object **must** say whether it is verified, by which rule, and otherwise why not. An
  organizer method is verified only when the message's authenticated sender is the organizer (or its
  `SENT-BY` within the organizer's organisational domain); an attendee method only when it is the replying
  attendee (or its `SENT-BY` within that domain).
- **FR-CAL-6** A `calendar.received` event **must** be emitted when a message with a calendar object first
  becomes visible as `received`, and never while it is quarantined, in screening, hidden or throttled. It
  **must** be thin, and an endpoint that receives it **must** be created only by a key with
  `messages:read`.
- **FR-CAL-7** Every text value of the object **must** be cleaned of control and hidden characters and
  treated as untrusted content.
- **FR-CAL-8** Send, reply and draft send **must** accept a caller-supplied iCalendar object, bounded in
  size, and **must** refuse it unless it has exactly one known `METHOD`, one `UID`, a `DTSTAMP` in every
  component and the per-method structure of RFC 5546, and unless its organizer (organizer methods) or
  replying attendee (attendee methods) is the address the message will be sent from, and its recipients
  are the object's attendees (organizer methods) or its organizer (attendee methods).
- **FR-CAL-9** A calendar message **must** be sent as iMIP: the human-readable text and a
  `text/calendar` part whose `method` parameter equals `METHOD`, as alternatives, optionally with an
  `application/ics` copy. Missing `VTIMEZONE`s for zones that resolve by name **must** be added, other
  systems' extension properties removed and reported, and the object re-serialised with CRLF and 75-octet
  folding. A calendar message **must** be `kind: transactional` and **must never** be sent through domain
  fallback.
- **FR-CAL-10** An identity **must** be able to answer a verified invitation it is an attendee of with
  `accepted`, `declined` or `tentative` and an optional comment, through one idempotent route that sends an
  iTIP `REPLY` to the organizer from the attendee address in the invitation's thread, and refuses
  unverified, quarantined, cancelled, superseded, past and non-attendee cases with defined errors.
- **FR-CAL-11** An identity **must not** be able to send, for one `UID`, an organizer method with a lower
  `SEQUENCE` than one it already sent, also when two sends race.
- **FR-CAL-12** A verified calendar message without thread headers **must** join the thread that holds a
  calendar message with the same `UID`; an unverified one **must never** join by `UID`.
- **FR-CAL-13** Pylota Mail **must not** keep calendar state beyond the message's own row and attachment.
  Calendar data **must** be retained, erased (including by counterparty erasure of any address the object
  names), held and exported with its message.
- **FR-CAL-14** The calendar object **must** be readable, and the reply route usable, through the REST API,
  MCP and the CLI; the console **must** show the object read-only.

## 17. Edge cases

Section U of the register. Owner `S` is Pylota Mail, `S+I` both (the integrator acts on the result).

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| U1 | One invitation carried twice: an inline `text/calendar; method=REQUEST` part and an `invite.ics` (`application/ics`) attachment with the same object (Google's form) | One object; `other_objects` 0; `attachment_id` is the inline iMIP part's; the copy stays an attachment | S | `conf::calendar::u1_duplicate_parts`, `core::calendar::select_parts`, `live::calendar::inbound_real_invitations` |
| U2 | Outlook invitation whose `TZID` is a Windows name (`GMT Standard Time`, `W. Europe Standard Time`) | Mapped through CLDR's `territory="001"` table to the IANA zone; UTC instants computed; `tz_resolution: "windows"`. Every one of the table's names maps to a zone `chrono-tz` parses | S | `conf::calendar::u2_outlook_windows_tz`, `core::ical::windows_zones_table`, `core::ical::tz_resolution_order`, `live::calendar::inbound_real_invitations` |
| U3 | `TZID` that is `/`-prefixed (`/mozilla.org/…/Europe/London`), known only from the object's `VTIMEZONE`, defined with a rule outside the supported subset, or unknown with no definition | Prefixed: the IANA suffix (`prefixed`). Definition only: the offset from the definition (`vtimezone`). Unsupported definition or none: `utc: null`, warning `vtimezone_unsupported` or `unknown_tzid`, local time kept; verification and the event are unaffected | S | `conf::calendar::u3_tzid_forms`, `core::ical::tz_resolution_order`, `core::ical::vtimezone_subset` |
| U4 | A named zone whose `VTIMEZONE` disagrees with the tzdb; a local time in a daylight-saving gap or overlap | The tzdb wins, warning `tz_definition_mismatch`. An overlapped time is the first occurrence; a gap time uses the offset before the gap (RFC 5545 §3.3.5) | S | `conf::calendar::u4_tz_mismatch_and_dst`, `core::ical::dst_gap_overlap` |
| U5 | All-day event; floating time; no `DTEND` and no `DURATION`; `DTEND` before `DTSTART` | All-day: `date` values, exclusive end, `all_day: true`. Floating: `utc: null`, `floating: true`. Missing end: one day for a date, the start instant for a date-time (`source: "default"`). End before start: end = start, warning `end_before_start`. `ends_at` read in the tenant's time zone (`ends_at_basis: "tenant_timezone"`) for dates, floating and unknown zones | S | `conf::calendar::u5_all_day_floating`, `core::calendar::times_and_ends` |
| U6 | An object of overrides only (a `CANCEL` of one occurrence); two components with one `RECURRENCE-ID`; an override whose `RECURRENCE-ID` type differs from `DTSTART`; components with different `UID`s | Primary = the earliest `RECURRENCE-ID`; duplicates keep the higher `SEQUENCE`, then later `DTSTAMP` (`duplicate_override`); a mistyped override is dropped (`override_type_mismatch`); other `UID`s are ignored (`uid_mismatch`) | S | `conf::calendar::u6_instance_only_cancel`, `core::calendar::primary_and_overrides` |
| U7 | `RRULE` with unknown parts, `COUNT` and `UNTIL` together, out-of-range values, `INTERVAL` 5,000, `FREQ=SECONDLY`, two `RRULE`s, 600 bytes of text | `rrule.valid: false` for the invalid ones (the object stays `parsed`); `rrule_high_frequency`; the second rule ignored (`multiple_rrule`); text cut (`rrule_text_truncated`). Never expanded | S | `conf::calendar::u7_rrule_cases`, `core::ical::rrule_validation` |
| U8 | Google recurring invitation with edited instances (master plus `RECURRENCE-ID` overrides); 150 overrides; 1,200 overrides | Read whole: master at the top level, overrides sorted. 150: the first 100 kept, `overrides_truncated`, `overrides_total: 150`. 1,200: `too_complex`. AgentMail imports none of these | S | `conf::calendar::u8_override_bundle`, `core::calendar::primary_and_overrides`, `core::ical::bounds`, `live::calendar::inbound_real_invitations` |
| U9 | Object with only `VTODO`, `VJOURNAL` or `VFREEBUSY` (a free/busy request) | `object_status: "unsupported"`, `ignored_components` lists them, `verified: false`, `reason: "no_vevent"`; the event is still emitted | S | `conf::calendar::u9_no_vevent`, `core::calendar::methods_and_components` |
| U10 | Malformed object: no `END:VCALENDAR`, a bad date, invalid UTF-8, two `METHOD` lines, a missing `UID`; a part that makes the reader panic | `object_status: "invalid"` (invalid UTF-8 alone is a warning), `reason: "object_invalid"`; the message is stored and delivered as usual. A panic: degraded parsing from the third attempt skips step 10a and stores the message without its object; never dropped. The fuzz target finds no panic | S | `conf::calendar::u10_malformed`, `core::ical::content_lines`, `it::calendar::degraded_parse_keeps_message`, fuzz `ical_parse` |
| U11 | A 3 MiB calendar part; 25,000 content lines; a 100 KiB line; 600 components; 1,200 distinct party addresses | Over 1 MiB: `too_large` (`object_too_large`); the others `too_complex` (`object_too_complex`); work stays linear; the message is stored; the event is emitted with `verified: false` | S | `conf::calendar::u11_oversize`, `core::ical::bounds` |
| U12 | `VALARM` (an `EMAIL` alarm listing addresses), `ATTACH` (a URL and inline base64), `X-ALT-DESC` and other `X-` properties; tag characters, bidirectional overrides and zero-width characters in `SUMMARY`, `DESCRIPTION` and `CN` | Alarms, attachments and extension properties are absent from the object and counted in `removed`; hidden characters removed, flag `hidden_text`; strings returned as untrusted; nothing is fetched | S | `conf::calendar::u12_removed_and_hidden`, `core::calendar::text_cleaning`, `core::calendar::methods_and_components` |
| U13 | Two distinct objects in one message; 12 calendar parts; a `winmail.dat` carrying an `.ics`; a TNEF meeting written as MAPI properties only | First by the order of §2.1, `other_objects` counts the rest; parts after the 10th skipped (`parts_skipped`); the TNEF `.ics` is read; the MAPI-only meeting has no object and the flag `tnef_meeting_not_converted` | S | `conf::calendar::u13_parts_and_tnef`, `core::calendar::select_parts` |
| U14 | A forwarded invitation inside a nested `message/rfc822`; a DSN returning one of our invitations | Never read: the outer message has no object unless it has its own calendar part; a DSN or MDN is never read | S | `conf::calendar::u14_nested_and_dsn`, `core::calendar::select_parts` |
| U15 | `text/calendar` part's `method` differs from `METHOD`; no `method` parameter; no `METHOD` at all (an exported `.ics`); `METHOD:X-FOO` | `method_mismatch` (not verified); warning `method_param_missing`; `method: "none"`, verified as `publish`; `method: "unknown"` (`method_unknown`) | S | `conf::calendar::u15_method_cases`, `core::calendar::methods_and_components`, `core::calendar::verify_matrix` |
| U16 | 300 attendees; attendees with `urn:uuid:` or `invalid:nomail` addresses; `MAILTO:Jo%40Example.NET?subject=x`; the identity named under an old, retired address | The identity's own entries first, then the rest up to 100 (`attendees_truncated`, `attendees_total: 300`); non-`mailto` addresses `null` with `uri_scheme`, never matching; the `mailto` normalised (warning `mailto_hfields`); `is_self` true for any status of the identity's addresses | S | `conf::calendar::u16_attendees_and_addresses`, `core::calendar::parties`, `core::calendar::shrink_order` |
| U17 | Forged organizer: `attacker@evil.example` passes DMARC for its own domain and sends a `REQUEST` or `CANCEL` whose `ORGANIZER` is `ceo@victim.example` | `verified: false`, `reason: "sender_not_organizer"`; never answered, never imported by Pylota, never joins by `UID` | S+I | `core::calendar::verify_matrix`, `it::calendar::verification_end_to_end` |
| U18 | The organizer's address in `From` but authentication `fail`, `unverified`, `softfail` or `unaligned`, or several `From` addresses | No authenticated sender: `sender_not_authenticated` (and the message is usually quarantined, so no event until release) | S | `core::calendar::verify_matrix`, `it::calendar::verification_end_to_end` |
| U19 | `SENT-BY`: an assistant at the organizer's domain; a sender at another domain naming itself as `SENT-BY` | Same organisational domain: verified, `verified_by: "sent_by"`. Other domain: `sent_by_unverifiable` | S | `core::calendar::verify_matrix` |
| U20 | `REPLY` from the attendee it names; a `REPLY` with two `ATTENDEE`s; a `REPLY` sent by someone else; a delegate's `REPLY` (`DELEGATED-FROM`); a `COUNTER` from one of its attendees; a `REFRESH` | Attendee's own: verified (`attendee`). Two attendees: `attendee_count`. Someone else: `sender_not_attendee`. Delegate: verified as itself, `delegated_from` returned. `COUNTER` and `REFRESH` by the same rules | S+I | `core::calendar::verify_matrix` |
| U21 | An attendee forwards the organizer's `REQUEST` to the identity (RFC 5546 §3.2.2.6); a delegator sends one with `DELEGATED-TO` naming the identity | Not verified: `sender_not_organizer`, or `sender_is_delegator` for the delegation | S | `core::calendar::verify_matrix` |
| U22 | A domain with no DMARC record but an aligned DKIM signature; another workspace's identity on the shared platform domain; a test tenant's loopback delivery | Aligned without a record: authenticated (verdict `none`, `aligned_by: "dkim"`). Platform domain: verified when the identity is the organizer. Loopback: verified (`verdict: pass`) | S | `core::calendar::verify_matrix`, `it::calendar::verification_end_to_end`, `it::calendar::loopback_round_trip` |
| U23 | No `ORGANIZER`; a non-`mailto` organizer; components naming different organizers | `organizer_missing`, `organizer_not_mailto`, `organizer_inconsistent`; never verified | S | `core::calendar::verify_matrix` |
| U24 | Invitation stored `quarantined` (`auth_failed`, `spam`, `risky_attachment`), in `screening`, `hidden` or `throttled` | No `calendar.received` while held; the object is visible only to `quarantine:review`. A release appends it with `released: true` and the ingest-time verification; the end of screening appends it; `hidden` and `throttled` never | S | `it::calendar::held_mail_withholds_event` |
| U25 | A re-parse after a reader fix | `calendar.received` again with `reprocessed: true` only when the stored object changed; thread membership unchanged | S | `it::calendar::reparse_reemits_when_changed` |
| U26 | The same raw invitation delivered twice; one invitation to two identities of a tenant | One row and one event per mailbox; each copy has its own `is_self` and event; `is_primary_recipient` on one | S+I | `it::calendar::two_identities_and_redelivery` |
| U27 | An endpoint subscribing to `calendar.received` created by a key without `messages:read`; the system identity receiving an invitation; a partner endpoint | `403 permission_denied` (`details.required`); the system identity's mailbox emits none; partner and tenant endpoints with an identity filter receive it like any mailbox event | S | `it::calendar::webhook_permission_and_routing` |
| U28 | An attendee's `REPLY` that has no thread token, `In-Reply-To` or `References` | Joins the thread holding the invitation by `UID` (verified only), after token and header steps; `thread_join_unverified` when the sender is not a participant | S | `it::calendar::uid_thread_join`, `core::thread::calendar_uid_step` |
| U29 | A stranger reuses a known `UID` in an unverified object | Never joins by `UID`: headers or a new thread | S | `it::calendar::uid_thread_join`, `core::thread::calendar_uid_step` |
| U30 | `ORGANIZER` differs from the resolved `From`: another identity's address, an explicit `from_address`, a fallback-pinned thread (platform address), a `retiring` address not on the thread; a `REPLY` whose attendee is not the `From`; an identity trying to delegate with a `REQUEST` | `400 calendar_organizer_mismatch` or `400 calendar_attendee_mismatch` with `details.from_address`; nothing stored or reserved | S | `core::calendar::outbound_parties`, `it::calendar::organizer_binding` |
| U31 | The sending domain fails, is suspended, or has `delivery_events: manual` after a calendar message was accepted | `failed`, `domain_failing_no_fallback`, never sent from the platform address; the `sends` hold settled with nothing consumed. A held domain holds it as usual | S | `it::calendar::never_falls_back` |
| U32 | Payloads with no `METHOD`, two, `X-FOO`; a `VTODO`; no `UID`; differing `UID`s; no `DTSTAMP`; `SEQUENCE` −1; `PUBLISH` with attendees; `REQUEST` without attendees or `DTSTART`; `REPLY` with two attendees; `VALARM` in `CANCEL`; `ADD` with `SEQUENCE:0`; no `PRODID`; a syntax error | `400 calendar_invalid` with the `details.reason` of §5.2 (and `details.line` for a syntax error); nothing stored | S | `core::calendar::outbound_validation`, `it::calendar::send_validation_errors` |
| U33 | `calendar.ics` of 300 KiB; a 250 KiB object with large attachments that pass 5 MiB composed | `413 calendar_too_large` (`details.limit`); `413 message_too_large` counting both copies | S | `core::calendar::outbound_validation`, `it::calendar::send_request_mime` |
| U34 | A `TZID` used without a `VTIMEZONE`: `Europe/London` (Kairos's output), `GMT Standard Time`, `Mars/Olympus` | IANA and Windows names: a `VTIMEZONE` generated (reported in `added_vtimezones`) that gives the tzdb offsets for the covered years; unknown: `400 calendar_invalid` (`unknown_tzid`) | S | `core::ical::vtimezone_generate`, `core::calendar::normalise_outbound`, `it::calendar::vtimezone_completion`, `live::calendar::client_matrix` |
| U35 | Payload with LF line ends, 300-character lines, non-ASCII text, `X-KAIROS-REF` and `X-PYLOTA-…` properties, and `X-WR-TIMEZONE` | Re-serialised with CRLF and 75-octet folding (no UTF-8 sequence split), `charset=UTF-8`, `7bit` or `base64`; extension properties removed and listed in `removed`, `X-WR-TIMEZONE` kept; the `method` parameter equals `METHOD` | S | `core::ical::write_round_trip`, `core::calendar::normalise_outbound`, `core::compose::calendar_mime`, `it::calendar::send_request_mime` |
| U36 | A `REQUEST` to an address that is not an attendee; a `REPLY` to two recipients, or to someone other than the organizer; a `PUBLISH` to anyone | `400 calendar_recipient_mismatch` (`details.address`) for the first two; `PUBLISH` accepted | S | `core::calendar::outbound_validation`, `it::calendar::send_validation_errors` |
| U37 | A `REQUEST` with `SEQUENCE:2` after the identity sent `SEQUENCE:3` for the `UID`; two sends with `SEQUENCE` 4 and 3 racing through submit; the same `SEQUENCE` re-sent | `409 calendar_sequence_stale` (`details.latest_sequence`, `details.latest_message_id`); in the race the lower one committing second is refused and leaves no object; equal `SEQUENCE` accepted | S | `it::calendar::stale_sequence_race` |
| U38 | `calendar` with `kind: marketing` or `auto_reply`; on reply-all or forward; with neither `text` nor `html` | `400 invalid_request` with path `kind`, `calendar` or the content rule's path | S | `core::calendar::outbound_validation`, `it::calendar::send_validation_errors` |
| U39 | Invitation to an attendee the identity never wrote to (`require_known_recipient: true`); to a suppressed or send-blocked attendee | Accepted; those deliveries `suppressed` (`policy: unknown_recipient`, `suppression`, `send_block`); a dry run reports them; all suppressed: the message ends `suppressed` | S+I | `it::calendar::known_recipient_and_filters` |
| U40 | Calendar send from a `cloudflare`-transport domain (the platform domain included) before S1 records the raw path; after S1, with two recipients when raw takes one | `422 transport_unavailable`, `calendar_needs_raw_mime`; afterwards, if one recipient per raw send: `400 too_many_recipients` (`calendar_single_recipient`). `ses` and `smtp` domains send normally | S | `it::calendar::cloudflare_raw_path` |
| U41 | A calendar send retried with the same key and body; with the same key and another `ics`; a transport timeout | Replay with `deduplicated: true`; `409 idempotency_conflict`; `uncertain`, never resent | S | `it::calendar::send_idempotency_and_uncertain` |
| U42 | Forwarding a message that carries an invitation | The calendar part is re-attached as `application/ics` without `method`; the forward is not an iMIP message | S | `core::compose::calendar_mime`, `it::calendar::forward_attaches_plain_ics` |
| U43 | Calendar reply to a verified `REQUEST` the identity is invited to | `202`; one `REPLY` to the `ORGANIZER` only, from the attendee address, in the invitation's thread with `In-Reply-To`; `SEQUENCE` echoed when non-zero; `PARTSTAT` set; `COMMENT` escaped; subject `Accepted: …`; the organizer applies it | S | `core::calendar::reply_builder`, `it::calendar::reply_route_sends_reply`, `live::calendar::reply_applied` |
| U44 | Calendar reply to a message with no object, an `invalid` object, a `REPLY`, `CANCEL` or `PUBLISH`, or an outbound message | `409 calendar_not_invitation` (`details.method`, `details.object_status`) | S | `it::calendar::reply_route_refusals` |
| U45 | Calendar reply to an unverified invitation; to a quarantined one with `quarantine:review`; to a quarantined one without it; to a hidden or throttled one | `409 calendar_not_verified` with the object's reason; `409 calendar_not_verified` (`message_quarantined`); `404 message_not_found` for the last two | S | `it::calendar::reply_route_refusals` |
| U46 | The invitation is `STATUS:CANCELLED`; a later verified `CANCEL` for the whole event, or for the instance answered, has arrived; a later verified `REQUEST` with a higher `SEQUENCE`, or the same `SEQUENCE` and a later `DTSTAMP`, has arrived | `409 calendar_cancelled` (`details.cancel_message_id`); `409 calendar_superseded` (`details.latest_message_id`); an unverified later `CANCEL` changes nothing | S | `it::calendar::reply_route_refusals` |
| U47 | The event ended; an answered instance ended; a recurrence with `UNTIL` in the past; an open-ended recurrence; `recurrence_id` of the wrong type or in `EXDATE` | `409 calendar_event_past` (`details.ended_at`) for the first three; the open-ended one is accepted; `400 invalid_request` (path `recurrence_id`, or `instance_excluded`) | S | `core::calendar::times_and_ends`, `it::calendar::reply_route_refusals` |
| U48 | The identity is not an attendee; its attendee address is retired, pending, or the thread is fallback-pinned to the platform address | `409 calendar_not_attendee`; `409 calendar_attendee_address_unavailable` (`details.reason`) | S | `it::calendar::reply_route_refusals` |
| U49 | Accept, then decline within the same second with a new key; the same key twice; no key | Two `REPLY`s, the second with a strictly later `DTSTAMP`; a replay with `deduplicated: true` and nothing sent; `400 idempotency_key_required` | S | `core::calendar::reply_builder`, `it::calendar::reply_dtstamp_and_keys` |
| U50 | Calendar reply from a paused identity, a suspended tenant, a sending-paused tenant, an identity without an owner; over the plan's `sends`, a daily cap, the hop limit, `RL_SEND`, a busy thread; with a key without `messages:send`; for another identity's or tenant's message | The standard send errors in the standard order (`403 tenant_suspended` before `409 identity_paused`, `402 billing_limit`, `429 daily_cap_reached`, `409 loop_detected`, `429 rate_limited`, `409 thread_busy`); `403 permission_denied`; `404` exactly as for a missing message | S | `it::calendar::reply_route_standard_refusals`, `it::security::cross_tenant_matrix` |
| U51 | Counterparty erasure of an attendee or organizer address that appears only in an invitation's `calendar_json` (not in `From`, `To` or `Cc`), also by `CounterpartyHash` after a restore; message, thread, identity and tenant erasure; a held thread | The message, its object and its `.ics` are erased with receipt counts and the probe finds nothing; a held thread keeps them | S | `it::calendar::erasure_and_holds` |
| U52 | Subject-access export; logs of a run with invitations | `messages.json` has `calendar` (without `parties`); no `UID`, summary, location, description or attendee address in any log line | S | `it::calendar::export_and_logs`, `it::logs::i5_no_content_in_logs` |
| U53 | Calendar send and calendar reply with the `sends` allowance spent; an invitation to three attendees; inbound invitations when the plan is exhausted | `402 billing_limit`, safe to retry with the same key; three units held and consumed per submitted recipient; inbound always stored and evented | S | `it::calendar::billing_and_caps` |
| U54 | Test tenant: identity A sends a `REQUEST` to identity B of the same tenant; B answers with the reply route; a simulator recipient | B gets a verified `calendar.received` (loopback); A gets B's verified `REPLY` in the same thread with `organized_here: true`; simulator scripts apply | S | `it::calendar::loopback_round_trip` |
| U55 | Invitation to a paused identity; to an identity of a tenant suspended for less than 5 days | Paused: stored and evented (a paused identity receives); the reply route answers `409 identity_paused`. Suspended: the temporary failure of [A6](../edge-cases.md), no event | S | `it::calendar::paused_and_suspended` |

## 18. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::ical::content_lines` | Unfolding of CRLF and bare LF followed by a space or tab, including a fold inside a multi-octet UTF-8 sequence; quoted parameter values with `:`, `;` and `,`; text unescaping of `\\`, `\;`, `\,`, `\n`, `\N` and an unknown escape; invalid UTF-8 replaced with the warning; case-insensitive names | §2.2, U10 |
| `core::ical::bounds` | Every row of the bounds table gives its status at the bound and one past it (1 MiB, 20,000 lines, 64 KiB line, 50 parameters, 1,024-byte value, 500 components, 1,000 overrides, 1,000 attendees, 20 `VTIMEZONE`s, 10,000 `EXDATE`s, 3 levels, 1,024-byte `UID`, 1,000 parties); a 1 MiB object parses within 1 s native | §2.2, U8, U11 |
| `core::ical::rrule_validation` | Each rule of §2.6: valid rules; every invalid shape; the `INTERVAL`, `COUNT` and `BY…` list bounds; high-frequency warning; a second `RRULE` ignored; text cut at 512 bytes | §2.6, U7 |
| `core::ical::tz_resolution_order` | `Z`, IANA (any case), Windows (with collapsed spaces), `/`-prefixed, definition-only and unknown `TZID`s resolve in the order of §2.5; a named zone with a definition uses the tzdb | §2.5, U2, U3 |
| `core::ical::windows_zones_table` | The generated table has the 142 `territory="001"` names of the pinned CLDR file, each value parses with `chrono_tz::Tz::from_str`, and the five spot values of §2.5 are exact | §2.5, U2 |
| `core::ical::vtimezone_subset` | Offsets from definitions with yearly `BYMONTH`/`BYDAY` and `BYMONTHDAY` rules, `UNTIL`, `COUNT` and `RDATE`s, across both transitions; any other rule is `vtimezone_unsupported` | §2.5, U3 |
| `core::ical::dst_gap_overlap` | `TZID=Europe/London:20270328T013000` (a gap) reads with the offset before the gap; `20271031T013000` (an overlap) is the first occurrence; the same for a definition-only zone | §2.5, U4 |
| `core::ical::vtimezone_generate` | For `Europe/London`, `America/New_York`, `Australia/Sydney` (southern hemisphere), `Asia/Kolkata` (no DST) and a Windows name, the generated definition evaluated by `vtimezone_subset` gives the tzdb offset at every hour of the covered years; at most 8 KiB | §5.3, U34 |
| `core::ical::write_round_trip` | Writing a parsed object and reading it again gives an equal object; lines are CRLF, at most 75 octets, never split inside a UTF-8 sequence; text escaping round-trips | §5.3, U35 |
| `core::calendar::select_parts` | The selection order of §2.1; identical objects counted once; distinct ones in `other_objects`; the 10-part limit; TNEF files; nested messages, DSNs and MDNs never read; the MAPI-only TNEF flag | §2.1, U1, U13, U14 |
| `core::calendar::primary_and_overrides` | Primary selection with and without a master; override sorting, the 100 kept; duplicate, mistyped and other-`UID` components | §2.4, U6, U8 |
| `core::calendar::times_and_ends` | Time values for every form; end derivation (`dtend`, `duration`, `default`); end before start; `ends_at` for one-off, `UNTIL`, `RDATE`-only, `COUNT` and unbounded events, in UTC and in the tenant's time zone; the past check for an instance | §2.5, U5, U47 |
| `core::calendar::methods_and_components` | Every row of the methods table; the `method` parameter rules; ignored components; `unsupported`; removed alarms, attachments and extension properties | §2.3, U9, U12, U15 |
| `core::calendar::parties` | `mailto` normalisation (case, percent-encoding, query); non-`mailto` values; roles, statuses, types and defaults; `is_self` for every address status; attendee truncation order; `parties` HMACs | §2.7, U16 |
| `core::calendar::text_cleaning` | Bounds and truncation flags of every text field; control and hidden characters removed; the `hidden_text` flag rule | §2.8, U12 |
| `core::calendar::verify_matrix` | Every row of §3.2 and every reason of §3.3 in precedence order, with authentication `pass`, `none` with and without alignment, `unaligned`, `softfail`, `unverified`, `fail`, loopback and several `From` addresses; `SENT-BY` within and across organisational domains; delegates, delegators and forwards | §3, U15, U17–U23 |
| `core::calendar::shrink_order` | An object over 262,144 bytes is cut in the order of §4.1 and never loses its identifying fields or `parties` | §4.1, U16 |
| `core::calendar::outbound_validation` | Every row of step 9a's structure table, the size limit, the `kind` and operation rules, and the recipient rules of step 13a | §5.2, U32, U33, U36, U38 |
| `core::calendar::outbound_parties` | Organizer and attendee binding against the resolved `From`, case-insensitively, with the error details | §5.2, U30 |
| `core::calendar::normalise_outbound` | Extension properties and parameters removed except `X-WR-TIMEZONE`, at most 20 names reported; definitions added for IANA and Windows names; everything else unchanged; deterministic bytes for the same input | §5.3, U34, U35 |
| `core::calendar::reply_builder` | The `REPLY` of §6.3 for each `partstat`, with and without `SEQUENCE`, `RECURRENCE-ID` (date and date-time, with its `VTIMEZONE`) and `COMMENT`; `DTSTAMP` = max(now, latest + 1 s); the subject and text; the output passes `outbound_validation` | §6.3, U43, U49 |
| `core::compose::calendar_mime` | The MIME tree of §5.5 with and without HTML, attachments and the copy; `method` parameter equals `METHOD`; `component=VEVENT`; encodings; the forward form (`application/ics`, no `method`) | §5.5, §5.11, U35, U42 |
| `core::thread::calendar_uid_step` | `resolve_inbound` joins by `UID` only for a verified object, after token and headers, and sets `join_unverified` for a non-participant | §4.3, U28, U29 |
| `conf::calendar::u*` | One corpus case per inbound row U1–U16 under `crates/conformance/corpus/calendar/` (authored or derived from RFC examples, addresses under RFC 2606 names), each with its expected object; also run through workerd by `it::conformance::corpus_via_workerd` | U1–U16 |
| fuzz `ical_parse` | No panic; every output string is valid UTF-8; status is one of the five; time per input under 1 s; `write(parse(x))` re-parses to the same object | U10 |
| `it::calendar::degraded_parse_keeps_message` | A calendar part that panics the reader (a fault hook) leaves the message stored without its object at the third attempt, with `parse_degraded` | U10 |
| `it::calendar::verification_end_to_end` | Through `email()` with the DNS and DKIM fixtures: a passing organizer invitation is verified; a forged organizer, a failing `From` and several `From` addresses are not; an aligned sender without a DMARC record is | U17, U18, U22 |
| `it::calendar::held_mail_withholds_event` | A quarantined, screening, hidden or throttled invitation emits no `calendar.received`; release emits it once with `released: true` and the ingest-time verification; the screening verdict emits it | U24 |
| `it::calendar::reparse_reemits_when_changed` | A re-parse with a changed reader emits `reprocessed: true` once; an unchanged object emits nothing; the thread is unchanged | U25 |
| `it::calendar::two_identities_and_redelivery` | One invitation to `bookings` and `compliance` gives two rows and two events with their own `is_self`; the same raw again gives nothing more | U26 |
| `it::calendar::webhook_permission_and_routing` | Endpoint create, re-point and replay with `calendar.received` need `messages:read`; tenant, partner and platform endpoints and identity filters receive it; the system identity's mailbox emits none; the event carries no summary or address | U27, §4.4 |
| `it::calendar::uid_thread_join` | A header-less verified `REPLY` joins the invitation's thread; an unverified object with the same `UID` starts a new thread; a non-participant's verified join is flagged | U28, U29 |
| `it::calendar::send_request_mime` | A Kairos-shaped `REQUEST` sent through the SES and SMTP fakes arrives as the MIME of §5.5; the stored row has `calendar_json` with `verified: null`; the size limits | U33, U35 |
| `it::calendar::send_validation_errors` | Each payload of U32, U36 and U38 gets its error and stores, reserves and locks nothing | U32, U36, U38 |
| `it::calendar::organizer_binding` | Each case of U30, including a fallback-pinned thread whose error names the platform address | U30 |
| `it::calendar::never_falls_back` | A calendar message whose domain turns `failing`, `suspended` or `manual` before transport ends `failed` (`domain_failing_no_fallback`) and the hold is released; a non-calendar message in the same state falls back | U31 |
| `it::calendar::vtimezone_completion` | A payload with `TZID=Europe/London` and no definition is sent with a generated one and `added_vtimezones`; `Mars/Olympus` is refused | U34 |
| `it::calendar::stale_sequence_race` | The cases of U37, the race forced by pausing the first submit between its policy step and its accept transaction | U37 |
| `it::calendar::known_recipient_and_filters` | The cases of U39, and the dry run's recipient report | U39 |
| `it::calendar::cloudflare_raw_path` | Before the S1 result is wired in, a calendar send on a `cloudflare` domain gets `422` (`calendar_needs_raw_mime`); with the raw path, the `EmailMessage` call carries the composed MIME, and the single-recipient rule applies when configured | U40 |
| `it::calendar::send_idempotency_and_uncertain` | The cases of U41 with the simulator's `timeout@` | U41 |
| `it::calendar::forward_attaches_plain_ics` | A forward of an invitation carries `invite.ics` as `application/ics` without `method` | U42 |
| `it::calendar::reply_route_sends_reply` | The `202`, the stored message, the recipient, `From`, thread, headers and `REPLY` bytes of U43; the organizer is a known recipient | U43 |
| `it::calendar::reply_route_refusals` | Each case of U44–U48 with its code and details, in the order of §6.2 | U44–U48 |
| `it::calendar::reply_dtstamp_and_keys` | The cases of U49 under a controlled clock | U49 |
| `it::calendar::reply_route_standard_refusals` | Each case of U50, with the policy order | U50 |
| `it::calendar::erasure_and_holds` | The cases of U51, with receipt counts and probes | U51 |
| `it::calendar::export_and_logs` | `messages.json` includes `calendar` without `parties`; captured logs of the suite contain none of the canary `UID`, summary or attendee addresses | U52 |
| `it::calendar::billing_and_caps` | The cases of U53 | U53 |
| `it::calendar::loopback_round_trip` | The flow of U54 | U54, U22 |
| `it::calendar::paused_and_suspended` | The cases of U55 | U55 |
| `it::security::cross_tenant_matrix` | Gains `POST …/calendar-reply`: another tenant's or partner's identity and message answer `404` exactly as missing ones | U50 |
| `it::mcp::calendar_tools` | `mail_calendar_reply` is listed only for keys with `messages:send` and maps to the route with the same errors; `mail_send` and `mail_reply` accept `calendar`; `mail_get_message` caps attendees at 20 and overrides at 10 with the flags | §13 |
| `it::console::calendar_panel` | The panel for each role, escaped text, workspace-time-zone times, the verified badge and reason, the `.ics` link through the console route; a quarantined message shows method and badge only; no answer buttons | §13 |
| `cli::calendar::reply_and_send_flags` | `pmail calendar-reply` and `--calendar-file`, `--calendar-copy`, `--no-calendar-copy` map to the API fields; `--json` prints the object unchanged | §13 |
| `live::calendar::inbound_real_invitations` | Real invitations from Gmail and from Outlook (with a Windows `TZID` and a recurring series with an edited instance) arrive verified with correct UTC times | U1, U2, U8 |
| `live::calendar::client_matrix` | A `REQUEST`, an update and a `CANCEL` sent to Gmail, Outlook.com, Microsoft 365 and Apple Calendar each show one invitation at the right local time, in winter and summer; the result for the `invite.ics` copy and the generated `VTIMEZONE` is recorded on this page | U33, U34, §5.12 |
| `live::calendar::reply_applied` | A calendar reply to a Gmail and an Outlook invitation updates the organizer's attendee status | U43 |

## 19. Milestone

To add to [Build plan](../build-plan.md) after M31:

> ## M32 · Calendar invitations (after M7, M8, M9, M14, M15, M16 and M21)
>
> **Goal:** read, verify and expose inbound iCalendar, send iMIP from caller-supplied objects, and answer
> verified invitations, with no calendar store ([Calendar invitations](design/calendar-invitations.md)).
>
> **Files:** `crates/core/src/ical/{mod.rs, lex.rs, value.rs, rrule.rs, tz.rs, windows_zones.rs,
> vtimezone.rs, write.rs}`, `crates/core/src/calendar.rs`, `crates/worker/src/mailbox/calendar.rs`,
> `handlers/calendar_reply.rs`, `xtask/src/cldr.rs`, `fuzz/fuzz_targets/ical_parse.rs`,
> `crates/conformance/corpus/calendar/`, `crates/cli/src/commands/calendar_reply.rs`; no migration (the two
> `messages` columns and their index are in mailbox schema v1). Hooks in other milestones' files, each
> reviewed by that file's owner:
>
> | File | Hook |
> |---|---|
> | `consumers/inbound.rs` (M7) | Step 10a (calendar), the step 12 address query, `IngestInput.calendar` |
> | `core::classify` (M2, M7) | Row 5 reads the calendar object; step 10a upgrades `normal` to `calendar` |
> | `mailbox/ingest.rs`, `core::thread` (M7) | The `UID` step of `resolve_inbound`, `organized_here`, the two columns, `calendar.received` in the outbox; the same event in the release transaction and in `Reparse` |
> | `webhooks/payloads.rs`, `handlers/webhooks.rs` (M8) | The `calendar_received` builder and its golden file; `calendar.received` in the J29 permission table |
> | `handlers/send.rs`, `core::policy`, `core::compose`, `mailbox/submit.rs`, `transport/cloudflare.rs`, `consumers/outbound.rs` (M9) | The `calendar` field; policy steps 9a, 13a and 13b; recipient rule 5; the iMIP MIME; the stale-sequence re-check in the accept transaction; the raw Cloudflare path or its refusal; no fallback for calendar messages; `forward`'s `application/ics` form; `SendOp::CalendarReply` |
> | `jobs/erasure.rs` and the mailbox `ErasurePlan` (M14) | Counterparty matching on `calendar_json.parties`; `calendar` in the export's `messages.json` |
> | `mcp/tools.rs` (M15) | `mail_calendar_reply`; `calendar` on `mail_send` and `mail_reply`; the output caps |
> | `crates/cli/src/commands/send.rs` (M16) | `--calendar-file`, `--calendar-copy`, `--no-calendar-copy` |
> | `console/pages/thread.rs` (M21) | The calendar panel |
> | The draft send (M29) and the screening verdict (M31) | Whichever of M29, M31 and M32 lands later adds the `calendar` draft field and the `calendar.received` append at the end of screening |
>
> **Spike:** S1 gains the raw calendar case of [Calendar invitations §5.6](design/calendar-invitations.md#56-transports).
> Until its result is recorded, M32 ships with calendar sends on the `cloudflare` transport refused
> (`calendar_needs_raw_mime`).
>
> **Implements:** FR-CAL-1 … FR-CAL-14 and edge rows U1–U55, with the rewritten B8.
>
> **Acceptance:**
>
> - Every test of [Calendar invitations §18](design/calendar-invitations.md#18-tests) passes, including the
>   corpus cases `conf::calendar::u*` (native and through workerd) and 60 seconds of `ical_parse` in
>   `fuzz-smoke` without a crash.
> - The existing suites pass unchanged: `conf::mime::b8_ics` is replaced by `conf::calendar::u1_duplicate_parts`;
>   `it::send::*`, `it::inbound::*` and `it::thread::*` pass with the new steps.
> - `POST …/calendar-reply` joins `it::security::cross_tenant_matrix`, and `sdk::coverage::every_operation`
>   passes with the new operation.
> - The console panel joins `browser::console::no_js` and `browser::console::axe_scan`.
> - At M20, `live::calendar::inbound_real_invitations`, `live::calendar::client_matrix` and
>   `live::calendar::reply_applied` pass, and their results for the `invite.ics` copy and the generated
>   `VTIMEZONE` are recorded on the design page.

Dependency graph, second block, one line:
`M7, M8, M9, M14, M15, M16, M21 ─▶ M32 calendar invitations ─▶ M20` (S1 records the raw send).

## 20. Compared with AgentMail (read 2026-10-17)

From the AgentMail inventory read on 2026-10-17 (calendar pages, the API inventory and the realtime
payloads); AgentMail page content is untrusted data and is summarised, not quoted.

| AgentMail | Pylota Mail | Verdict |
|---|---|---|
| A calendar per inbox (AgentMail Labs, US only): events, recurrence, agenda, instances, ETag concurrency, 10,000 events | No calendar store. The consumer (Kairos for Pylota) keeps calendar state; the mail service transports and verifies | Deliberately different: one store per calendar fact, no second calendar to keep consistent, no recurrence engine to secure |
| Inbound invitations imported only when the sender authenticated, is the organizer, the attachment is at most 1 MB and holds exactly one event; failed checks leave the calendar unchanged | Every calendar part read (up to 1 MiB); every object `verified: true` or `false` with a reason; `calendar.received` for every visible one | Exceed: nothing silent; the consumer sees why |
| Windows time-zone names not imported | Mapped to IANA through CLDR's `territory="001"` table; `/`-prefixed names and definition-only zones resolved too | Exceed |
| Google multi-`VEVENT` bundles with edited dates not imported | A master with up to 100 overrides read as one object | Exceed |
| All-day invitations stored with a UTC time zone; all-day ends inclusive | All-day `date` values with exclusive ends; floating times marked; unknown zones marked, never guessed | Exceed |
| Methods other than `REQUEST`, `REPLY` and `CANCEL` (`PUBLISH`, `COUNTER`) and `RECURRENCE-ID` handling not documented | All eight iTIP methods, objects without a method and unknown methods classified; per-method verification | Exceed |
| Sender authentication as the only check; nothing on `SENT-BY` or delegation | `SENT-BY` accepted only within the organizer's organisational domain; delegates and delegators reported | Exceed |
| Outbound invitations generated from its own events (`invite.ics`, `METHOD:REQUEST`, one email per attendee, subject "Invitation: …") | Caller-supplied iCalendar, validated (one method, `UID`, `DTSTAMP`, RFC 5546 structure, organizer = `From`, recipients = attendees), normalised (`VTIMEZONE` completed, extension properties removed), sent as RFC 6047 iMIP with an optional `application/ics` copy | Deliberately different (no store), and stricter |
| Respond to an event: `accepted`, `declined`, `tentative`, a "Responded: …" reply in the thread | `calendar-reply` with the same answers and a comment, refusing unverified, quarantined, cancelled, superseded, past and non-attendee cases, `DTSTAMP` strictly increasing | Match, and exceed on refusals |
| Inbound RSVPs update attendee status on its event | Verified `REPLY` objects with `organized_here`, joined to the invitation's thread by `UID`; the consumer applies them | Deliberately different: the consumer owns the status |
| Sequence ordering by iCalendar `SEQUENCE` inside its store | The consumer orders versions; Pylota Mail refuses an identity's own stale `SEQUENCE` and answers to superseded or cancelled invitations | Deliberately different, with a guard |
| Webhooks `calendar.event.created`, `updated`, `deleted`, `responded`, `starting`, `ending` (needs `calendar_event_read`) | One thin `calendar.received` (needs `messages:read`); starting and ending reminders are the consumer's | Deliberately different: no store, so no store events |
| Invites, updates, cancellations and replies count against send limits per recipient | Calendar sends and replies take the `sends` hold per recipient that is not suppressed | Match |
| Calendar permissions (`calendar_read` … `calendar_event_delete`) | No new permission: `messages:read`, `messages:send`, `attachments:read` | Deliberately different: calendar mail is mail |
| Labs gating, US only | On every deployment and region, no gating | Exceed |

## 21. Integration checklist

Make these edits, in this order, once PR #5 has merged. Text in quotation blocks is to be pasted as
written.

1. **`docs/src/SUMMARY.md`**: no change; it already lists `- [Calendar invitations](project/design/calendar-invitations.md)`
   after "Screening hold for agent-steering mail".

2. **`project/design/index.md`**: in the Index table, after the Screening row (or after "Testing" when
   it is not there yet), add:

   ```markdown
   | [Calendar invitations](calendar-invitations.md) | Reading, bounding and verifying inbound iCalendar (iMIP, iTIP) into the message's `calendar` object, time-zone resolution, the `calendar.received` event, threading by `UID`, iMIP sends from caller-supplied objects with organizer binding and the stale-sequence guard, and the `calendar-reply` route. No calendar store. |
   ```

   In the Spikes table, row **S1**, append to "Must prove":
   > ; and a raw `EmailMessage` send of a calendar message (`multipart/alternative` with `text/plain` and
   > `text/calendar; method=REQUEST; charset=UTF-8`) to two arbitrary recipients: whether it is accepted,
   > whether one `EmailMessage` can carry several envelope recipients, whether the `text/calendar` part and
   > its `method` parameter arrive unchanged, and whether Cloudflare keeps the composed `Message-ID`
   ([Calendar invitations §5.6](calendar-invitations.md#56-transports))

   and to "Fallback":
   > Calendar sends on the `cloudflare` transport stay refused with `422 transport_unavailable`
   > (`calendar_needs_raw_mime`); with one recipient per raw send, they are limited to one recipient
   > (`calendar_single_recipient`).

   and to "Affects": ", [Calendar invitations](calendar-invitations.md)". Make the same additions to the S1
   row of `project/build-plan.md` (M1): the first to "Prove", the second to "If it fails".

3. **`project/prd.md`**:
   - Non-goals (v1.0), add the bullet:
     > - A calendar. Pylota Mail keeps no events, agenda, recurrence engine or reminders; it reads and
     >   verifies iCalendar in mail, sends iMIP built by the integrator, and answers verified invitations
     >   on request ([Calendar invitations](design/calendar-invitations.md)).
   - §5 scope table: in the **Inbound** row, P0 column, append `; calendar objects with verification and
     calendar.received (FR-CAL-1–7, FR-CAL-12, FR-CAL-13)`; in the **Outbound** row, P1 column, append
     `; iMIP sends, the stale-sequence guard and calendar replies (FR-CAL-8–11), on the Cloudflare transport
     once spike S1 records the raw send`; in the **Integrations** row, P0 column, append `; the calendar
     object on REST, MCP, the CLI and the console (FR-CAL-14)`, and replace its P1 cell (`–`) with
     `calendar-reply on REST, mail_calendar_reply and pmail calendar-reply (FR-CAL-10, FR-CAL-14)`.
   - §6, new subsection `### 6.N Calendar invitations` after the last subsection present when the edit is
     made (N is the next free number: the Drafts and Agent self-sign-up checklists both name 6.16, so the
     pages that land first take the lower numbers), with the fourteen FR-CAL rows of
     [§16](#16-functional-requirements) as written.
   - FR-THR-1: replace the list with:
     > 1. a valid thread token in the recipient sub-address;
     > 2. `In-Reply-To` or `References` matching a stored message;
     > 3. the `UID` of a verified calendar object matching a calendar message of the mailbox;
     > 4. otherwise it starts a new thread.

4. **`reference/api.md`**:
   - Idempotency, first bullet: "**Required** on `POST …/messages`, `…/reply`, `…/reply-all`, `…/forward`
     and `…/calendar-reply`."
   - Permissions table, `messages:send` row: "Send, reply, reply-all, forward, cancel, calendar replies".
   - Message object: add `"calendar": null` to the JSON after `"kind"`, and the bullet:
     > - `calendar` is the message's calendar object, or `null`: method, verification (`verified`,
     >   `verified_by`, `reason`), `uid`, `sequence`, `dtstamp`, `recurrence_id`, `status`, `summary`,
     >   `description` (an excerpt), `location`, `start` and `end` (UTC instants with the original `TZID` and
     >   how it was resolved), `all_day`, `floating`, `ends_at`, `rrule` (validated, never expanded),
     >   `exdates_count`, `organizer`, `attendees` (with `partstat`, `role`, `rsvp`, `is_self`), overrides,
     >   `organized_here` and the `attachment_id` of the raw `.ics`. All its strings are untrusted content.
     >   Alarms, attachments and extension properties are never included
     >   ([Calendar invitations §2.9](../project/design/calendar-invitations.md#29-the-calendar-object)).
   - `POST /v1/identities/{identity_id}/messages`, add after the `headers` bullet:
     > - `calendar`: `{ "ics": "<iCalendar>", "attach_copy": true }` sends iMIP. `ics` is at most 262,144
     >   bytes (`413 calendar_too_large`) and needs exactly one `METHOD`, one `UID`, a `DTSTAMP` in each
     >   `VEVENT` and the structure RFC 5546 gives its method (`400 calendar_invalid` with
     >   `details.reason`). For `PUBLISH`, `REQUEST`, `ADD`, `CANCEL` and `DECLINECOUNTER` the `ORGANIZER`
     >   must be the address the message is sent from (`400 calendar_organizer_mismatch`, which names it)
     >   and every recipient an `ATTENDEE`; for `REPLY`, `REFRESH` and `COUNTER` the replying `ATTENDEE`
     >   must be the `From` (`400 calendar_attendee_mismatch`) and the one recipient the `ORGANIZER`
     >   (`400 calendar_recipient_mismatch` otherwise). `kind` must be `transactional`. Missing
     >   `VTIMEZONE`s are added and extension properties other than `X-WR-TIMEZONE` removed (both listed in
     >   `calendar.outbound`). A lower `SEQUENCE` than one this identity already sent for the `UID` gets
     >   `409 calendar_sequence_stale`. A calendar message never falls back to the platform domain; on a
     >   `cloudflare`-transport domain it gets `422 transport_unavailable` (`calendar_needs_raw_mime`) until
     >   the raw send is available ([Calendar invitations §5](../project/design/calendar-invitations.md#5-outbound-sending-icalendar)).
   - `…/reply`: add "It also accepts `calendar`, as `send` does."; `…/reply-all` and `…/forward`: add
     "`calendar` is refused (`400 invalid_request`)."
   - New section after `…/forward`:
     > ### `POST /v1/identities/{identity_id}/messages/{message_id}/calendar-reply` — `messages:send`
     >
     > ```json
     > { "partstat": "accepted", "comment": "Our driver will bring the car at 08:45.", "recurrence_id": null }
     > ```
     >
     > Answers a verified invitation (`method: "request"`) that names this identity as an attendee: an iTIP
     > `REPLY` goes to the invitation's `ORGANIZER` only, from the attendee's address, in the invitation's
     > thread. `partstat` is `accepted`, `declined` or `tentative`; `comment` at most 1,000 characters;
     > `recurrence_id` (an RFC 3339 instant, or a date for an all-day invitation) answers one instance.
     > `Idempotency-Key` is required, and `?dry_run=true` works as on sends. Returns `202` with the Message
     > object. Refusals: `409 calendar_not_invitation`, `409 calendar_not_verified` (also for a quarantined
     > invitation: release it first), `409 calendar_not_attendee`, `409 calendar_cancelled`,
     > `409 calendar_superseded`, `409 calendar_event_past`, `409 calendar_attendee_address_unavailable`,
     > and every send error ([Calendar invitations §6](../project/design/calendar-invitations.md#6-the-calendar-reply-route)).

5. **`reference/openapi.yaml`**: add the path, the schemas and the error codes:

   ```yaml
   paths:
     /identities/{identity_id}/messages/{message_id}/calendar-reply:
       parameters:
         - $ref: '#/components/parameters/IdentityIdPath'
         - $ref: '#/components/parameters/MessageIdPath'
       post:
         operationId: replyToCalendarInvitation
         tags: [Sending]
         summary: Answer a calendar invitation
         description: >-
           Sends an iTIP REPLY with the given participation status to the ORGANIZER of a verified invitation
           (method `request`) that names this identity as an attendee, from the attendee's address, in the
           invitation's thread. Idempotency, `dry_run`, policy and response behave as in `replyToMessage`.
           Refused for an unverified, quarantined, cancelled, superseded or past invitation, or when the
           identity is not an attendee ([U43]–[U50]).
         x-required-permission: ['messages:send']
         x-key-levels: [platform, partner, tenant, identity]
         x-permission-notes: 'The invitation must be visible to the key: a `quarantined` one needs `quarantine:review` and is then refused with `calendar_not_verified`; a `hidden` or `throttled` one is `404 message_not_found`.'
         x-rate-limit-buckets: [api, send]
         x-idempotency: required
         parameters:
           - $ref: '#/components/parameters/IdempotencyKeySend'
           - $ref: '#/components/parameters/DryRun'
         requestBody:
           required: true
           content:
             application/json:
               schema: {$ref: '#/components/schemas/CalendarReplyRequest'}
               example: {partstat: accepted, comment: Our driver will bring the car at 08:45.}
         responses:
           '200': {$ref: '#/components/responses/DryRunOk'}
           '202':
             description: Accepted into the queue, or the original response when the request is a replay.
             headers:
               Request-Id: {$ref: '#/components/headers/Request-Id'}
               RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
               Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
             content:
               application/json:
                 schema: {$ref: '#/components/schemas/SendResponse'}
           '400': {$ref: '#/components/responses/SendBadRequest'}
           '401': {$ref: '#/components/responses/Unauthorized'}
           '402': {$ref: '#/components/responses/BillingLimit'}
           '403': {$ref: '#/components/responses/SendForbidden'}
           '404': {$ref: '#/components/responses/NotFound'}
           '409': {$ref: '#/components/responses/CalendarReplyConflict'}
           '422': {$ref: '#/components/responses/SendUnprocessable'}
           '429': {$ref: '#/components/responses/SendTooManyRequests'}
           '500': {$ref: '#/components/responses/InternalError'}
           '503': {$ref: '#/components/responses/ServiceUnavailable'}
           '504': {$ref: '#/components/responses/GatewayTimeout'}
   components:
     responses:
       CalendarReplyConflict:
         description: >-
           The invitation cannot be answered, or the send conflicts with the idempotency record or with the
           identity, thread or domain state (as `SendConflict`).
         x-error-codes: [calendar_not_invitation, calendar_not_verified, calendar_not_attendee, calendar_cancelled, calendar_superseded, calendar_event_past, calendar_attendee_address_unavailable, idempotency_conflict, request_in_progress, identity_paused, identity_owner_required, thread_busy, domain_not_ready, loop_detected, sending_paused]
         headers:
           Request-Id: {$ref: '#/components/headers/Request-Id'}
         content:
           application/json:
             schema: {$ref: '#/components/schemas/Error'}
     schemas:
       CalendarReplyRequest:
         type: object
         required: [partstat]
         additionalProperties: false
         properties:
           partstat: {type: string, enum: [accepted, declined, tentative]}
           comment: {type: string, maxLength: 1000}
           recurrence_id:
             type: [string, 'null']
             description: An RFC 3339 instant, or `YYYY-MM-DD` when the invitation's DTSTART is a date.
       CalendarSendInput:
         type: object
         required: [ics]
         additionalProperties: false
         description: An iCalendar object to send as iMIP ([Calendar invitations §5]).
         properties:
           ics: {type: string, maxLength: 262144, description: 'UTF-8 iCalendar text; the limit is in bytes, checked by the server.'}
           attach_copy: {type: boolean, description: 'Default true for organizer methods, false for attendee methods.'}
       CalendarTime:
         type: object
         required: [utc, local, date, tzid, tz, tz_resolution]
         properties:
           utc: {type: [string, 'null'], format: date-time}
           local: {type: [string, 'null'], description: 'Local wall time, YYYY-MM-DDTHH:MM:SS.'}
           date: {type: [string, 'null'], format: date}
           tzid: {type: [string, 'null'], maxLength: 256}
           tz: {type: [string, 'null'], description: IANA zone name.}
           tz_resolution: {type: string, enum: [utc, iana, windows, prefixed, vtimezone, unknown, floating, date]}
           source: {type: string, enum: [dtend, duration, default], description: Ends only.}
       CalendarParty:
         type: object
         required: [address, name]
         properties:
           address: {type: [string, 'null']}
           name: {type: [string, 'null'], maxLength: 256}
           sent_by: {type: [string, 'null']}
           uri_scheme: {type: [string, 'null']}
       CalendarAttendee:
         allOf:
           - $ref: '#/components/schemas/CalendarParty'
           - type: object
             required: [role, partstat, rsvp, cutype, is_self]
             properties:
               role: {type: string, enum: [chair, req_participant, opt_participant, non_participant, other]}
               partstat: {type: string, enum: [needs_action, accepted, declined, tentative, delegated, other]}
               rsvp: {type: boolean}
               cutype: {type: string, enum: [individual, group, resource, room, unknown]}
               delegated_from: {type: array, maxItems: 5, items: {type: string}}
               delegated_to: {type: array, maxItems: 5, items: {type: string}}
               is_self: {type: boolean}
       MessageCalendar:
         type: object
         description: >-
           The message's calendar object ([Calendar invitations §2.9]). Every string is untrusted content.
           New `warnings` values and new `reason` values may be added within /v1.
         required: [object_status, method, verified, reason, attachment_id]
         properties:
           object_status: {type: string, enum: [parsed, invalid, too_large, too_complex, unsupported]}
           method: {type: [string, 'null'], enum: [publish, request, reply, add, cancel, refresh, counter, declinecounter, none, unknown, null]}
           verified: {type: [boolean, 'null'], description: 'null on outbound messages.'}
           verified_by: {type: [string, 'null'], enum: [organizer, attendee, sent_by, null]}
           reason: {type: [string, 'null'], enum: [object_invalid, object_too_large, object_too_complex, no_vevent, method_unknown, method_mismatch, organizer_missing, organizer_not_mailto, organizer_inconsistent, attendee_count, sender_not_authenticated, sender_is_delegator, sent_by_unverifiable, sender_not_organizer, sender_not_attendee, null]}
           authenticated_sender: {type: [string, 'null']}
           uid: {type: [string, 'null'], maxLength: 1024}
           sequence: {type: [integer, 'null']}
           dtstamp: {type: [string, 'null'], format: date-time}
           recurrence_id: {oneOf: [{$ref: '#/components/schemas/CalendarTime'}, {type: 'null'}]}
           status: {type: [string, 'null'], enum: [tentative, confirmed, cancelled, null]}
           summary: {type: [string, 'null'], maxLength: 1000}
           summary_truncated: {type: boolean}
           description: {type: [string, 'null'], maxLength: 2000}
           description_truncated: {type: boolean}
           location: {type: [string, 'null'], maxLength: 1024}
           comment: {type: [string, 'null'], maxLength: 2000}
           start: {oneOf: [{$ref: '#/components/schemas/CalendarTime'}, {type: 'null'}]}
           end: {oneOf: [{$ref: '#/components/schemas/CalendarTime'}, {type: 'null'}]}
           all_day: {type: boolean}
           floating: {type: boolean}
           ends_at: {type: [string, 'null'], format: date-time}
           ends_at_basis: {type: string, enum: [utc, tenant_timezone]}
           rrule:
             type: [object, 'null']
             properties:
               text: {type: string, maxLength: 512}
               valid: {type: boolean}
               freq: {type: [string, 'null']}
               interval: {type: [integer, 'null']}
               count: {type: [integer, 'null']}
               until: {type: [string, 'null']}
           exdates_count: {type: integer}
           rdates_count: {type: integer}
           organizer: {oneOf: [{$ref: '#/components/schemas/CalendarParty'}, {type: 'null'}]}
           attendees: {type: array, maxItems: 100, items: {$ref: '#/components/schemas/CalendarAttendee'}}
           attendees_total: {type: integer}
           attendees_truncated: {type: boolean}
           overrides:
             type: array
             maxItems: 100
             items:
               type: object
               properties:
                 recurrence_id: {$ref: '#/components/schemas/CalendarTime'}
                 this_and_future: {type: boolean}
                 sequence: {type: integer}
                 status: {type: [string, 'null']}
                 start: {$ref: '#/components/schemas/CalendarTime'}
                 end: {$ref: '#/components/schemas/CalendarTime'}
                 summary: {type: [string, 'null'], maxLength: 256}
                 location: {type: [string, 'null'], maxLength: 256}
                 self: {oneOf: [{$ref: '#/components/schemas/CalendarAttendee'}, {type: 'null'}]}
           overrides_total: {type: integer}
           overrides_truncated: {type: boolean}
           organized_here: {type: boolean}
           attachment_id: {type: [string, 'null']}
           other_objects: {type: integer}
           ignored_components: {type: array, items: {type: string}}
           removed:
             type: object
             properties:
               alarms: {type: integer}
               attachments: {type: integer}
               x_properties: {type: integer}
           warnings: {type: array, items: {type: string}}
           outbound:
             type: object
             description: Outbound messages only.
             properties:
               attach_copy: {type: boolean}
               added_vtimezones: {type: array, maxItems: 10, items: {type: string}}
               removed: {type: array, maxItems: 20, items: {type: string}}
           parser_version: {type: integer}
       CalendarReceivedData:
         type: object
         required: [message_id, thread_id, object_status, method, uid, sequence, recurrence_id, verified, verified_by, reason, organized_here]
         properties:
           message_id: {$ref: '#/components/schemas/MessageId'}
           thread_id: {$ref: '#/components/schemas/ThreadId'}
           object_status: {type: string}
           method: {type: [string, 'null']}
           uid: {type: [string, 'null'], maxLength: 1024}
           sequence: {type: [integer, 'null']}
           recurrence_id: {oneOf: [{$ref: '#/components/schemas/CalendarTime'}, {type: 'null'}]}
           verified: {type: boolean}
           verified_by: {type: [string, 'null']}
           reason: {type: [string, 'null']}
           organized_here: {type: boolean}
           released: {type: boolean}
           reprocessed: {type: boolean}
       CalendarReceivedEvent:
         description: '`calendar.received`: A message with a calendar object became visible as received. Read the object with `getMessage`.'
         allOf:
           - $ref: '#/components/schemas/EventBase'
           - type: object
             required: [type, data]
             properties:
               type:
                 const: calendar.received
               data: {$ref: '#/components/schemas/CalendarReceivedData'}
   webhooks:
     calendar.received:
       post:
         operationId: onCalendarReceived
         tags: [Events]
         summary: 'calendar.received'
         description: 'A message with a calendar object became visible as received: when stored, released, or re-parsed with a changed object. Never for quarantined, screening, hidden or throttled mail.'
         security: []
         parameters:
           - $ref: '#/components/parameters/WebhookIdHeader'
           - $ref: '#/components/parameters/WebhookTimestampHeader'
           - $ref: '#/components/parameters/WebhookSignatureHeader'
           - $ref: '#/components/parameters/WebhookUserAgentHeader'
         requestBody:
           required: true
           content:
             application/json:
               schema: {$ref: '#/components/schemas/CalendarReceivedEvent'}
         responses:
           '2XX': {$ref: '#/components/responses/WebhookAccepted'}
           '410': {$ref: '#/components/responses/WebhookGone'}
           default: {$ref: '#/components/responses/WebhookFailed'}
   ```

   Place the `webhooks` entry after `verification.received`. Also:
   - `EventType` `enum`: add `- calendar.received` after `- verification.received`; the event `oneOf` list:
     add `- $ref: '#/components/schemas/CalendarReceivedEvent'` after `VerificationReceivedEvent`; its
     discriminator `mapping`: add `calendar.received: '#/components/schemas/CalendarReceivedEvent'` after
     `verification.received`.
   - In the `x-permission-notes` of `createPlatformWebhook`, `createTenantWebhook`, `updateWebhook` and
     `replayWebhookEvents` (and wherever else the sentence appears), change "any `message.*` type or
     `verification.received`" to "any `message.*` type, `verification.received` or `calendar.received`".
   - `EventBase.sequence` description: add `calendar.received` after `verification.received` in the list of
     mailbox events.
   - `Message`: add `calendar: {oneOf: [{$ref: '#/components/schemas/MessageCalendar'}, {type: 'null'}]}`
     to `properties` and `calendar` to `required`.
   - `SendRequest` and `ReplyRequest`: add `calendar: {$ref: '#/components/schemas/CalendarSendInput'}`.
   - `SendBadRequest` `x-error-codes`: add `calendar_invalid, calendar_organizer_mismatch,
     calendar_attendee_mismatch, calendar_recipient_mismatch`, and append to its description:
     "`too_many_recipients` with `details.reason = "calendar_single_recipient"`: a calendar message on the
     `cloudflare` transport when a raw send takes one recipient."
   - `SendConflict` `x-error-codes`: add `calendar_sequence_stale`.
   - `SendPayloadTooLarge` `x-error-codes`: add `calendar_too_large`.
   - `SendUnprocessable` description: append "A calendar message on a `cloudflare` domain before the raw send
     is available gets `transport_unavailable` with `details.reason = "calendar_needs_raw_mime"`."
   - `sendMessage` and `replyToMessage` descriptions: append "`calendar` sends iMIP
     ([Calendar invitations §5])."; `replyAllToMessage` and `forwardMessage`: append "`calendar` is
     refused."

6. **`reference/errors.md`**:
   - Validation table, add:
     > | 400 | `calendar_invalid` | no | A `calendar.ics` that cannot be read, or whose structure is not a valid iTIP message: `details.reason` is one of `parse_error` (with `details.line`), `version_missing`, `prodid_missing`, `method_missing`, `multiple_methods`, `method_unknown`, `component_not_supported`, `uid_missing`, `uid_mismatch`, `uid_too_long`, `dtstamp_missing`, `sequence_invalid`, `organizer_missing`, `organizer_inconsistent`, `attendee_not_allowed`, `attendee_missing`, `attendee_count`, `dtstart_missing`, `summary_missing`, `property_not_allowed` (with `details.property`), `rrule_invalid`, `unknown_tzid` (with `details.tzid`) ([U32](../project/edge-cases.md)) |
     > | 400 | `calendar_organizer_mismatch` | no | The object's `ORGANIZER` is not the address the message would be sent from. `details.organizer` and `details.from_address` (on a fallback-pinned thread, the platform address) ([U30](../project/edge-cases.md)) |
     > | 400 | `calendar_attendee_mismatch` | no | A `REPLY`, `REFRESH` or `COUNTER` whose replying `ATTENDEE` is not the `From` address. `details.attendee`, `details.from_address` |
     > | 400 | `calendar_recipient_mismatch` | no | An organizer method sent to someone who is not an `ATTENDEE`, or an attendee method not sent to exactly the `ORGANIZER`. `details.address` ([U36](../project/edge-cases.md)) |
     > | 413 | `calendar_too_large` | no | `calendar.ics` is over 262,144 bytes. `details.limit` |
   - The `too_many_recipients` row: append "With `details.reason = "calendar_single_recipient"` and
     `details.max = 1`, a calendar message on the `cloudflare` transport, when a raw send takes one
     recipient ([Calendar invitations §5.6](../project/design/calendar-invitations.md#56-transports))."
   - Resources and state table, add:
     > | 409 | `calendar_sequence_stale` | no | An organizer method whose `SEQUENCE` is lower than one this identity already sent for the same `UID`. `details.sequence`, `details.latest_sequence`, `details.latest_message_id` ([U37](../project/edge-cases.md)) |
     > | 409 | `calendar_not_invitation` | no | `calendar-reply` on a message that has no readable invitation (`method: "request"`), or that is outbound. `details.method`, `details.object_status` |
     > | 409 | `calendar_not_verified` | no | `calendar-reply` on an invitation that is not verified (`details.reason` is its reason) or that is quarantined (`details.reason = "message_quarantined"`; release it first) |
     > | 409 | `calendar_not_attendee` | no | `calendar-reply` on an invitation that does not name this identity as an attendee |
     > | 409 | `calendar_cancelled` | no | `calendar-reply` on an invitation that is cancelled, or that a later verified `CANCEL` covers. `details.cancel_message_id` when known |
     > | 409 | `calendar_superseded` | no | `calendar-reply` on an invitation that a later verified `REQUEST` replaced. `details.latest_message_id`, `details.latest_sequence`; answer that one |
     > | 409 | `calendar_event_past` | no | `calendar-reply` for an event or instance that has ended. `details.ended_at` |
     > | 409 | `calendar_attendee_address_unavailable` | no | `calendar-reply` when the attendee address cannot be the `From`: `details.address`, `details.reason` (`retired`, `pending`, `fallback_pinned`) |
   - The `transport_unavailable` row: add `calendar_needs_raw_mime` (a calendar message on the
     `cloudflare` transport before the raw send is available) to the list of `details.reason` values.
   - Send failures after `202`, `domain_failing_no_fallback` row: append "A calendar message never falls
     back, whatever the policy."

7. **`reference/events.md`**:
   - Ordering: add `calendar.received` to the per-identity list ("per identity for mailbox events
     (`message.*`, `identity.*`, `verification.received`, `calendar.received`, …)").
   - Envelope section, the paragraph on mail events: "Because the `message.*`, `verification.received` and
     `calendar.received` payloads carry mail, …".
   - New section after Messages:
     > ### Calendar
     >
     > | Type | When | `data` |
     > |---|---|---|
     > | `calendar.received` | A message with a calendar object first becomes visible as `received`: when it is stored, released from quarantine or screening, or when a re-parse changed its object. Never for quarantined, screening, hidden or throttled mail, or the system identity | `message_id`, `thread_id`, `object_status`, `method`, `uid`, `sequence`, `recurrence_id`, `verified`, `verified_by`, `reason`, `organized_here`; `released: true` or `reprocessed: true` when it comes from a release or a re-parse. The `uid` is text the sender chose; read the object with `GET …/messages/{message_id}` |

8. **`reference/limits.md`**: new section after "Service sign-up ledger":
   > ## Calendar
   >
   > From [Calendar invitations](../project/design/calendar-invitations.md).
   >
   > | Limit | Value |
   > |---|---|
   > | Calendar parts read per message | 10 |
   > | Calendar part read | 1 MiB decoded; larger: `object_status: "too_large"` |
   > | Content lines; one unfolded line | 20,000; 64 KiB |
   > | Components per object | 500 |
   > | Overrides (`RECURRENCE-ID`) read; kept in the object | 1,000; 100 |
   > | Attendees read per component; kept | 1,000; 100 |
   > | Distinct party addresses per object | 1,000 |
   > | `VTIMEZONE`s; observances; `RDATE`s per observance | 20; 50; 100 |
   > | `EXDATE` and `RDATE` values per component | 10,000 each |
   > | `UID` | 1,024 bytes |
   > | `summary`, `location`, `description` excerpt, `comment`, names | 1,000, 1,024, 2,000, 2,000 and 256 characters |
   > | `RRULE` text; `INTERVAL`; `COUNT`; values per `BY…` part | 512 bytes; 1–1,000; 1–100,000; 366 |
   > | Stored calendar object | 262,144 bytes per message |
   > | Outbound `calendar.ics` | 262,144 bytes (`413 calendar_too_large`) |
   > | Generated `VTIMEZONE`s | 10 per message, 8 KiB each |
   > | Calendar-reply `comment` | 1,000 characters |
   >
   > In the Mail table, the "Stored message row" row: "… `text` 512 KiB, `html_sanitized` 1 MiB,
   > `extracted_text` 256 KiB, `calendar_json` 256 KiB, …".

9. **`reference/configuration.md`**: no change. Calendar adds no policy field and no deployment variable.

10. **`project/design/data-model.md`**: in `messages` (mailbox schema v1), after `metadata_json`, add the two
    columns (with a comma after `metadata_json`'s default) and, after `messages_status`, the index of
    [§4.1](#41-columns-and-caps) as written; in the Mailbox notes add 2 to "The widest table, `messages`,
    has 45 columns." (47 when no other parity page has added a column first). The `automated_json` comment
    stays as it is; its `calendar` member is no longer written.

11. **`project/design/inbound.md`**:
    - Parsing, replace the **Calendar (B8)** bullet with:
      > - **Calendar ([B8](../edge-cases.md)).** Calendar parts are read at consumer step 10a, after TNEF is
      >   unpacked, into the bounded, verified `calendar` object of
      >   [Calendar invitations](calendar-invitations.md). Invites are never answered or accepted by the
      >   service on its own.
    - Storage caps: in the `automated_json` row remove "; `calendar` strings 256 characters"; add the row
      `| calendar_json | 262,144 bytes, with the shrink order of [Calendar invitations §4.1](calendar-invitations.md#41-columns-and-caps) |`;
      change "at most about 270 KiB" to "at most about 526 KiB" and "about 820 KiB" to "about 560 KiB".
    - The consumer's steps: add after step 10:
      > 10a. **Calendar**: select, read and verify calendar parts; set `kind` to `calendar` when it was
      >      `normal` ([Calendar invitations §4.2](calendar-invitations.md#42-ingest)).
      and in step 12 the bullet:
      > - only when step 10a read a calendar object: the identity's addresses,
      >   `SELECT address FROM addresses WHERE identity_id = ?1`, for `is_self`
      >   ([Calendar invitations §4.2](calendar-invitations.md#42-ingest)).
    - Automated mail and loops, row 5: "A calendar object was read at step 10a, and none of the above" →
      `calendar`.
    - `IngestInput`: add `pub calendar: Option<CalendarInput>,   // step 10a; None without a calendar part`.
    - `ingest` step 9: the column list gains `calendar_json, calendar_uid_hash` (39 parameters); step 16:
      "plus `calendar.received` when the message has a calendar object, is `received`, and the mailbox is
      not the system identity's ([Calendar invitations §4.4](calendar-invitations.md#44-the-calendarreceived-event))".
    - Release, step 5: "Outbox: `message.released` …; and `calendar.received` with `released: true` when the
      message has a calendar object."
    - Re-parsing: add `calendar_json` and `calendar_uid_hash` to the updated columns, and "`calendar.received`
      again with `reprocessed: true` when the calendar object changed."
    - Tests: replace `conf::mime::b8_ics` with `conf::calendar::u*` in the B8 row.

12. **`project/design/outbound.md`**:
    - Policy pipeline: add rows **9a** ("`calendar` present: `kind: transactional`, operation `send` or
      `reply`, size, read, iTIP structure, normalisation ([Calendar invitations §5.2](calendar-invitations.md#52-validation-in-pipeline-order))"
      → `400 invalid_request`, `413 calendar_too_large`, `400 calendar_invalid`), **13a** ("calendar
      parties: `ORGANIZER` or replying `ATTENDEE` = the resolved `From`; recipients" →
      `400 calendar_organizer_mismatch`, `calendar_attendee_mismatch`, `calendar_recipient_mismatch`) and
      **13b** ("calendar sequence" → `409 calendar_sequence_stale`); in row 15 add "and a calendar message
      on the `cloudflare` transport before the raw send is available" → `details.reason =
      "calendar_needs_raw_mime"`.
    - Recipient filters, Known recipient, add rule 5 of [§5.4](#54-recipients-and-the-known-recipient-rule) (after the Drafts page's rule 4)
      as written.
    - Reservation inside the mailbox, step 7: "re-check that no `idempotency` row exists for the key, and,
      for a calendar message, re-run the stale-sequence query ([Calendar invitations §5.9](calendar-invitations.md#59-the-stale-sequence-guard)); …".
    - From address and fallback, the fallback row: append "A message with `calendar_json` never falls back:
      it takes the `domain_fallback = false` row ([Calendar invitations §5.6](calendar-invitations.md#56-transports))."
    - Body: append item 6: "**Calendar**: the iMIP structure of [Calendar invitations §5.5](calendar-invitations.md#55-mime-composition)."
    - Transports, `CloudflareTransport`: append "A calendar message always uses the raw form (`EmailMessage`
      with the composed MIME), and none is sent until spike S1 records that it works."
    - Body, item 5 (Forward): append "Calendar attachments are re-attached as `application/ics` without a
      `method` parameter ([Calendar invitations §5.11](calendar-invitations.md#511-forwarding-an-invitation))."
    - Idempotency fingerprint, the `target` comment: "… for reply, reply_all, forward and calendar_reply".

13. **`project/design/threading.md`**: §3.1, add step 3b of [§4.3](#43-threading-by-uid) as written, `JoinVia::CalendarUid`,
    `ThreadLookup::thread_of_calendar_uid` with its query, `InboundThreadInputs.calendar_uid_hash:
    Option<&str>` (set only for a verified object), and in the `join_unverified` list "or `CalendarUid`";
    §8 table, add `| U28, U29 | A verified calendar reply joins by UID; an unverified one never does | 3.1 |`;
    §9 tests, add `core::thread::calendar_uid_step` and `it::calendar::uid_thread_join`.

14. **`project/design/webhooks.md`**: builders table, add
    `| calendar_received(row) | calendar.received | message_id, thread_id, object_status, method, uid, sequence, recurrence_id, verified, verified_by, reason, organized_here (released, reprocessed) |`;
    J29 table, first row: "`["*"]`, or any `message.*` type, or `verification.received`, or
    `calendar.received`" → `messages:read`.

15. **`project/design/privacy.md`**: §6.4 first sentence, append "…, or is a party of the message's calendar
    object (the `parties` list of `calendar_json`, [Calendar invitations §10](calendar-invitations.md#10-privacy))";
    §4.1, after the `CounterpartyHash` sentence that ends "… and matches it against `hash`.", add: "Calendar
    objects keep no clear address: their `parties` entries are the first 16 bytes of that HMAC, so both
    targets compare the first 16 bytes of the target's hash with them ([Calendar invitations §10](calendar-invitations.md#10-privacy))."; §9.3, add `calendar` to the
    message record (without `parties`); §13, add `it::calendar::erasure_and_holds` and
    `it::calendar::export_and_logs`.

16. **`project/edge-cases.md`**: header: add 55 to the row count and 1 to the section count, add `U` to the
    section list, and add 55 to the Pylota-owned count (all 55 rows are `S` or `S+I`). On the register as
    of PR #5 (292 rows in 15 sections, 286 Pylota-owned) that reads "The register has 347 rows in 16
    sections (A–L, N, O, U and W). The 341 rows that Pylota Mail owns …"; recount when other parity
    sections land first. Replace row B8's behaviour with:
    > Calendar parts become a bounded, verified `calendar` object ([Calendar invitations](design/calendar-invitations.md));
    > invites are never answered or accepted by the service on its own. Read receipts (MDNs) are never sent
    and its test cell with `conf::calendar::u1_duplicate_parts`, `core::classify::b8_mdn_request_ignored`;
    add the section `## U · Calendar invitations` with the 55 rows of [§17](#17-edge-cases) as written.

17. **`project/build-plan.md`**: add the M32 milestone of [§19](#19-milestone) after M31; add the dependency
    line to the second graph block; in "Shared files", add "`mailbox/ingest.rs` and `core::thread`: M32 adds
    the `UID` step" and "`core::compose`: M32 adds the iMIP structure".

18. **`project/design/testing.md`**: §5.1 layout, add the line
    `    calendar/      u1_* … u16_*                                    calendar parts, time zones, methods, bounds`;
    §8, add the fuzz target row
    `| ical_parse | object_status is one of parsed, invalid, too_large, too_complex, unsupported; every output string is valid UTF-8; time per input under 1 s; for a parsed object, writing it and reading the result gives the same object |`,
    and add one to the count in "The five required by the edge-case work" and "CI runs the five" ("six"
    when no other parity page has added a target first).

19. **`project/design/security.md`**:
    - §3.1 (TB1), add the row:
      > | S | Spoofed organizer, cancellation or reply in a calendar invitation | Each calendar object is verified against the message's authenticated sender, with the rule or the reason; an unverified object is never answered by `calendar-reply` and never threaded by `UID` ([U17](../edge-cases.md)–[U23](../edge-cases.md)) | `it::calendar::verification_end_to_end` |
      > | D | Calendar part built to exhaust the parser (deep nesting, huge recurrence, thousands of overrides or time zones) | Bounded reader: 1 MiB, 20,000 lines, 500 components and the other bounds of [Calendar invitations §2.2](calendar-invitations.md#22-the-bounded-reader); recurrence never expanded ([U11](../edge-cases.md)) | `core::ical::bounds`, fuzz `ical_parse` |
    - §8.1, first sentence: "Everything from mail (headers, display names, subjects, bodies, filenames,
      attachment text, DSN text, calendar objects), …".
    - §8.4, append: "Calendar objects are read without fetching `TZURL`, `ATTACH`, `URL`, `ALTREP` or any
      other URI they carry ([Calendar invitations §9](calendar-invitations.md#9-security-and-visibility))."
    - §13, add `it::calendar::verification_end_to_end` and `it::security::cross_tenant_matrix` (the
      calendar-reply route).

20. **`project/design/mcp.md`**:
    - §3 filtering table, after the `mail_forward` row:

      ```markdown
      | `mail_calendar_reply` | `messages:send` | – |
      ```

    - §4 tools table, after the `mail_forward` row:

      ```markdown
      | `mail_calendar_reply` | `POST …/messages/{message_id}/calendar-reply` with `Idempotency-Key` | `RL_SEND` |
      ```

    - §4.1 annotations: the row "`mail_send`, `mail_reply`, `mail_forward`" becomes "`mail_send`,
      `mail_reply`, `mail_forward`, `mail_calendar_reply`".
    - §4.2 budgets, the `mail_get_message` and `mail_get_thread` rows: append "; `calendar.attendees` cut
      to 20 and `calendar.overrides` to 10 per object, with `attendees_truncated` and
      `overrides_truncated`".
    - §4.3: `#### mail_send` and `#### mail_reply` gain the optional argument `calendar`
      (`{ ics, attach_copy }`, as REST); add `#### mail_calendar_reply` after `#### mail_forward`, with
      the arguments of [§13](#13-console-mcp-and-cli) (`identity`, `message_id`, `partstat`,
      `idempotency_key` required; `comment`, `recurrence_id` optional) and the output of `mail_reply`.

    **`reference/mcp.md`**: Tools table, after the `mail_reply` row:

    ```markdown
    | `mail_calendar_reply` | Answers a verified calendar invitation: accepted, declined or tentative | `messages:send` | `POST …/calendar-reply` |
    ```

    `### mail_send` and `### mail_reply` gain `calendar`; add `### mail_calendar_reply` after
    `### mail_forward` with its arguments and this example:

    ```json
    { "identity": "maintenance.acme@agents.example", "message_id": "msg_01JA9B2C3D4EW7X2M5N6P8R0T0",
      "partstat": "accepted", "comment": "Our driver will bring the car at 08:45.",
      "idempotency_key": "brightwell-mot-accept" }
    ```

21. **`reference/cli.md`**: Sending, add `### calendar-reply` with the synopsis of [§13](#13-console-mcp-and-cli)
    and the example `pmail calendar-reply msg_01JA9B2C3D4EW7X2M5N6P8R0T0 --identity maintenance.acme@agents.example --partstat accepted --idempotency-key brightwell-mot-accept`;
    add `--calendar-file`, `--calendar-copy` and `--no-calendar-copy` to `send` and `reply` with the row
    "`--calendar-file` | An iCalendar file to send as iMIP (at most 256 KiB); `--no-calendar-copy` leaves out
    the `invite.ics` copy".

22. **`project/design/console.md`**: Showing untrusted mail, add the bullet "A message with a calendar object
    shows a read-only **Calendar** panel ([Calendar invitations §13](calendar-invitations.md#13-console-mcp-and-cli));
    it offers no answer buttons." and the test `it::console::calendar_panel` to the tests table.

23. **`project/design/observability.md`**: §3.2, add the eight metric rows of [§14](#14-observability);
    §5.3, add the alerts `calendar_parse_failures` and `calendar_raw_refused`; §2.2, add `calendar_parsed`
    and `calendar_send_refused`.

24. **`project/design/rust-workspace.md`**: the `core` row's responsibilities gain "the bounded iCalendar
    reader and writer and time-zone resolution (`core::ical`), calendar objects, verification and iMIP
    validation (`core::calendar`)"; the §9 `xtask` table gains the row

    ```markdown
    | `cargo xtask cldr-windows-zones --cldr <tag>` | Reads `common/supplemental/windowsZones.xml` at the pinned CLDR release tag and writes `crates/core/src/ical/windows_zones.rs`: the `territory="001"` entries only, each value resolved through the tzdb `backward` file when `chrono_tz::Tz::from_str` does not parse it, with the tag in the file header ([Calendar invitations §2.5](calendar-invitations.md#25-times-and-time-zones)) |
    ```

    the fuzz targets table gains the row

    ```markdown
    | `ical_parse` | The `core::ical` reader with the bounds of [Calendar invitations §2.2](calendar-invitations.md#22-the-bounded-reader), then the `core::calendar` object builder and the `core::ical` writer |
    ```

    and the `fuzz-smoke` row's target list gains `ical_parse`.

25. **Parity pages**: [Drafts](drafts.md) (writer A): the draft body gains `calendar`
    (`CalendarSendInput`), stored with the draft and validated at send by policy steps 9a, 13a and 13b;
    [Screening](screening.md) (writer C): the transaction that moves a message from `screening` to
    `received` appends `calendar.received` when the message has a calendar object
    ([§4.4](#44-the-calendarreceived-event)); [Event feed](event-feed.md) (writer B): no change, the event is
    fed like every other.

26. **Guides**: `guides/receiving.md`, the calendar row: "`kind: calendar` with a verified, structured
    `calendar` object and a `calendar.received` event; never accepted automatically"; `guides/agents.md`,
    the B8 row: "Never accept calendar invitations automatically; answer verified ones with
    `calendar-reply` only when the operator wants it. Never send read receipts"; `guides/sending.md`, a
    short "Calendar invitations" section pointing to `calendar` on send and to the reply route.

