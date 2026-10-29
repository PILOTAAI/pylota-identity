//! `sdk::coverage::every_operation` (FR-SDK-1, Rust workspace § 11): the SDK has one method per
//! `openapi.yaml` operation, and only the operations meant for other callers are left out.

use std::collections::BTreeSet;

/// The operations the SDK leaves out (`x-sdk: false`): the provider hooks that only Amazon SNS and
/// Telnyx call, the security text file for people, and the vault consumer's own routes, which the
/// consumer's client calls.
const NOT_IN_SDK: &[&str] = &[
    "receiveSesNotification",
    "receiveSesInboundNotification",
    "receiveTelnyxEvent",
    "getSecurityTxt",
    "getVaultConsumerSelf",
    "redeemVaultTicket",
    "reportVaultOutcome",
    "writeBackVault",
    "getVaultUseStatusForConsumer",
];

#[test]
fn every_operation() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/src/reference/openapi.yaml"),
    )
    .expect("the contract");
    let doc: serde_json::Value = yaml_serde::from_str(&text).expect("valid YAML");
    let mut contract = BTreeSet::new();
    let mut skipped = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            if !["get", "post", "put", "patch", "delete"].contains(&method.as_str()) {
                continue;
            }
            let id = op["operationId"].as_str().unwrap().to_owned();
            if op["x-sdk"] == false {
                skipped.insert(id);
            } else {
                contract.insert((method.to_ascii_uppercase(), path.clone(), id));
            }
        }
    }
    let sdk: BTreeSet<(String, String, String)> = pylota_identity::OPERATIONS
        .iter()
        .map(|(m, p, id)| ((*m).to_owned(), (*p).to_owned(), (*id).to_owned()))
        .collect();
    let missing: Vec<_> = contract.difference(&sdk).collect();
    let extra: Vec<_> = sdk.difference(&contract).collect();
    assert!(
        missing.is_empty(),
        "operations without an SDK method: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "SDK methods without an operation: {extra:?}"
    );
    let allowed: BTreeSet<String> = NOT_IN_SDK.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(skipped, allowed, "x-sdk: false outside the allowed list");
}
