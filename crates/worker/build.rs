//! Passes the three build-time values that `cargo xtask build-worker` sets to the compiler
//! (Rust workspace § 9). Native and test builds get the defaults.

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    for (name, default) in [
        ("PM_BUILD_VERSION", version.as_str()),
        ("PM_BUILD_COMMIT", "unknown"),
        ("PM_BUILD_DATE", "1970-01-01"),
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name).unwrap_or_else(|_| default.to_owned());
        println!("cargo:rustc-env={name}={value}");
    }
}
