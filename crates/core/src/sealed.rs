//! The sealed-column registry ([Security §7.2]): every column that holds a `pm1.` envelope, with the
//! expression that gives its row ID in the AAD. The master-key rotation reads it to re-seal and count;
//! code that adds a sealed column adds its entry. Phone and vault columns join with their tables.
//!
//! [Security §7.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

/// One sealed column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sealed {
    pub table: &'static str,
    pub column: &'static str,
    /// SQL over the row giving the AAD's row ID.
    pub row_id: &'static str,
}

pub const SEALED: &[Sealed] = &[
    Sealed {
        table: "webhook_endpoints",
        column: "secret_enc",
        row_id: "id",
    },
    Sealed {
        table: "webhook_endpoints",
        column: "prev_secret_enc",
        row_id: "id",
    },
    Sealed {
        table: "webhook_endpoints",
        column: "headers_sealed",
        row_id: "id",
    },
    Sealed {
        table: "identity_keys",
        column: "private_enc",
        row_id: "id",
    },
    Sealed {
        table: "signing_keys",
        column: "ciphertext",
        row_id: "purpose || ':' || kid",
    },
    Sealed {
        table: "domains",
        column: "smtp_sealed",
        row_id: "id",
    },
    Sealed {
        table: "domains",
        column: "smtp_pending_sealed",
        row_id: "id",
    },
    Sealed {
        table: "users",
        column: "totp_sealed",
        row_id: "id",
    },
    Sealed {
        table: "users",
        column: "recovery_codes_sealed",
        row_id: "id",
    },
    Sealed {
        table: "oauth_states",
        column: "pkce_sealed",
        row_id: "state_hash",
    },
];

#[cfg(test)]
mod tests {
    use super::SEALED;

    /// Every registered column exists in `0001_init.sql`, in its table.
    #[test]
    fn registry_matches_schema() {
        let sql = include_str!("../../../migrations/d1/0001_init.sql");
        for s in SEALED {
            let start = sql
                .find(&format!("CREATE TABLE {} (", s.table))
                .unwrap_or_else(|| panic!("table {}", s.table));
            let body = &sql[start..start + sql[start..].find("\n);").unwrap()];
            assert!(
                body.lines()
                    .any(|l| l.trim_start().starts_with(&format!("{} ", s.column))),
                "{}.{}",
                s.table,
                s.column
            );
        }
    }
}
