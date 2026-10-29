//! `sdk::doctest::published_examples` (Rust workspace § 11): the landing page's Rust example is
//! `examples/confirm_booking.rs` byte for byte once its HTML is unescaped and its tags removed, and the
//! quickstart shows the same code. The example builds with `cargo build -p pylota-identity --examples`,
//! and `cargo test --doc` compiles the quickstart's Rust blocks.

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn published_examples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let page = std::fs::read_to_string(root.join("../../site/public/index.html"))
        .expect("the landing page");
    let open = "<code id=\"code-rust\">";
    let start = page.find(open).expect("the Rust block") + open.len();
    let end = start + page[start..].find("</code>").expect("its end");
    let shown = unescape(&strip_tags(&page[start..end]));
    let example =
        std::fs::read_to_string(root.join("examples/confirm_booking.rs")).expect("the example");
    assert_eq!(shown, example.strip_suffix('\n').unwrap_or(&example));
    let quickstart =
        std::fs::read_to_string(root.join("../../docs/src/quickstart.md")).expect("the quickstart");
    assert!(
        quickstart.contains(&shown),
        "the quickstart shows the same example"
    );
}
