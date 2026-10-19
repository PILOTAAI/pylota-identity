//! `pmail`, the Pylota Mail command-line tool (build plan M16). M0 only proves the binary builds.
#![forbid(unsafe_code)]

fn main() {
    println!("pmail {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    /// The crate builds and its test harness runs (build plan M0).
    #[test]
    fn workspace_member_builds() {}
}
