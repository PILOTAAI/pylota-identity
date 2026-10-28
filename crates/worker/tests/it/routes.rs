//! The router (Security § 5.1).

use crate::call;

#[test]
fn unknown_route_and_method() {
    let r = call("GET", "/v1/does-not-exist", None, None, &[]);
    assert_eq!((r.status, r.code()), (404, "route_not_found"));
    let r = call("PUT", "/v1/tenants", None, None, &[]);
    assert_eq!((r.status, r.code()), (405, "method_not_allowed"));
    assert_eq!(r.header("allow"), Some("POST, GET"));
    assert_eq!(call("GET", "/console/sign-in", None, None, &[]).status, 404);
}
