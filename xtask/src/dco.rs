//! `cargo xtask dco <base> <head>` (build plan M19, the `dco` job): every commit of `base..head` that
//! is not a merge carries a `Signed-off-by:` trailer naming its author exactly, `Name <email>`
//! (Developer Certificate of Origin 1.1).

use std::path::Path;
use std::process::Command;

pub struct Commit {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub parents: usize,
    pub message: String,
}

/// The commits without a matching sign-off.
pub fn unsigned(commits: &[Commit]) -> Vec<String> {
    commits
        .iter()
        .filter(|c| c.parents < 2)
        .filter(|c| {
            let want = format!("Signed-off-by: {} <{}>", c.author, c.email);
            !c.message.lines().any(|l| l.trim_end() == want)
        })
        .map(|c| {
            format!(
                "{} has no \"Signed-off-by: {} <{}>\"",
                &c.sha[..c.sha.len().min(12)],
                c.author,
                c.email
            )
        })
        .collect()
}

fn commits(root: &Path, range: &str) -> Result<Vec<Commit>, String> {
    let out = Command::new("git")
        .args(["log", "--format=%H%x00%an%x00%ae%x00%P%x00%B%x1e", range])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git log: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git log {range}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\x1e')
        .filter(|r| !r.trim().is_empty())
        .filter_map(|r| {
            let mut f = r.trim_start_matches('\n').splitn(5, '\0');
            Some(Commit {
                sha: f.next()?.to_owned(),
                author: f.next()?.to_owned(),
                email: f.next()?.to_owned(),
                parents: f.next()?.split_whitespace().count(),
                message: f.next()?.to_owned(),
            })
        })
        .collect())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let (Some(base), Some(head)) = (args.first(), args.get(1)) else {
        return Err("usage: cargo xtask dco <base> <head>".into());
    };
    let list = commits(root, &format!("{base}..{head}"))?;
    let bad = unsigned(&list);
    if bad.is_empty() {
        println!("dco: {} commit(s) signed off", list.len());
        Ok(())
    } else {
        Err(format!(
            "dco: sign off with `git commit -s` (DCO 1.1):\n  {}",
            bad.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(author: &str, email: &str, parents: usize, message: &str) -> Commit {
        Commit {
            sha: "0123456789abcdef".into(),
            author: author.into(),
            email: email.into(),
            parents,
            message: message.into(),
        }
    }

    /// `xtask::dco_refuses_unsigned` (build plan M19): a commit with no sign-off, or one that names
    /// someone else or another address, is refused; a sign-off matching the author passes, and merge
    /// commits are not checked.
    #[test]
    fn dco_refuses_unsigned() {
        let ok = "Fix a thing\n\nSigned-off-by: Ana Okafor <ana@example.invalid>\n";
        assert!(unsigned(&[c("Ana Okafor", "ana@example.invalid", 1, ok)]).is_empty());
        for (case, commit) in [
            ("none", c("Ana Okafor", "ana@example.invalid", 1, "Fix a thing\n")),
            ("another person", c("Ben Byrne", "ben@example.invalid", 1, ok)),
            ("another address", c("Ana Okafor", "ana@other.invalid", 1, ok)),
            ("in the body only", c("Ana Okafor", "ana@example.invalid", 1, "See Signed-off-by: Ana Okafor <ana@example.invalid> above\n")),
        ] {
            assert_eq!(unsigned(&[commit]).len(), 1, "{case}");
        }
        assert!(unsigned(&[c("GitHub", "noreply@github.com", 2, "Merge branch x\n")]).is_empty());
    }
}
