//! The automatic containment rules acted on outside the evaluator (Observability § 5.6). An owner check
//! that fails (`rpc::check_owner`, synchronous) marks the isolate; the entry point that ran it applies
//! `read_only` when it ends ([`end_invocation`]): the switch `on` (`set_by = 'auto'`), the
//! `rpc_owner_mismatch` report and the `containment:read_only` page.

use std::cell::Cell;

use pylota_identity_platform::Platform;
use serde_json::json;

thread_local! {
    static OWNER_MISMATCH: Cell<bool> = const { Cell::new(false) };
}

/// Called by the owner check on a mismatch.
pub fn note_owner_mismatch() {
    OWNER_MISMATCH.with(|m| m.set(true));
}

/// The end of every invocation: a pending containment, then the metrics flush.
pub async fn end_invocation<P: Platform>(p: &P) {
    if OWNER_MISMATCH.with(|m| m.replace(false)) {
        let r = crate::ops::switches::set(
            p,
            "read_only",
            "on",
            "auto",
            "rpc_owner_mismatch",
            None,
            "containment",
        )
        .await;
        if r.is_err() {
            // Not lost: the next mismatch tries again, and the metric and log line are written.
            crate::log::error("containment_failed", &[("detail", "read_only".into())]);
        }
        crate::ops::alerts::report(
            p,
            "rpc_owner_mismatch",
            None,
            None,
            &json!({}),
            "containment",
        )
        .await;
        crate::ops::alerts::report(
            p,
            "containment:read_only",
            None,
            None,
            &json!({ "trigger": "rpc_owner_mismatch" }),
            "containment",
        )
        .await;
        crate::metrics::count_with(p, "containment_total", None, &["read_only"], 1);
    }
    crate::metrics::flush(p);
}
