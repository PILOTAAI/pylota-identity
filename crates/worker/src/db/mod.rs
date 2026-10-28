//! The data-access layer over D1. Every statement is a literal with bound parameters
//! (`worker::db::no_formatted_sql`); tenant data is read with the tenant ID as a required parameter, and
//! list filters are written `(?n IS NULL OR column = ?n)` so the SQL never changes shape.

pub mod addresses;
pub mod audit;
pub mod domains;
pub mod idempotency;
pub mod identities;
pub mod intents;
pub mod jobs;
pub mod keys;
pub mod partners;
pub mod tenants;
pub mod webhooks;

#[cfg(test)]
mod tests {
    /// No SQL is built with `format!`: every `Stmt` holds a literal and bound values (Security § 8).
    #[test]
    fn no_formatted_sql() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let path = e.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, line) in text.lines().enumerate() {
                    let l = line.trim_start();
                    for kw in ["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE "] {
                        if (l.contains(&format!("format!(\"{kw}"))
                            || l.contains(&format!("format!(\"{}", kw.to_lowercase())))
                            && !l.contains("no_formatted_sql")
                        {
                            offenders.push(format!("{}:{}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "SQL built with format!: {offenders:?}"
        );
    }
}
