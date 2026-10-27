# S3 FTS5 in Durable Object SQLite: result

**Pass**, 2026-10-27, on the deployed runtime: the SQL in `statements.json` ran, one statement at a time, in
the SQLite storage of a deployed Durable Object (the `SpikeObject` of `spikes/s1-bindings`, route `/do/sql`,
Worker `pi-spike-2`).

| Check | Result |
|---|---|
| `fts` exactly as [Data model §2](../../docs/src/project/design/data-model.md#2-identitymailbox-durable-object-sqlite) defines it: six columns, `content = ''`, `contentless_delete = 1`, `tokenize = 'unicode61 remove_diacritics 2'` | Created |
| `fts_tri` with `tokenize = 'trigram'` | Created |
| `-bm25(fts, 8.0, 4.0, 3.0, 1.0, 1.5, 10.0)` | Works; ranks the message with "Fiat" in the subject and body above the one with it in the body only |
| Diacritics | `cafe` matches `Café` |
| Column filter, phrase | `subject:report` and `"BK 4821"` match as expected |
| Trigram substring | `"4821"` and `"vailab"` match on `fts_tri` |
| Contentless read | Column values read back as `NULL`, as for any contentless table |
| `DELETE FROM fts WHERE rowid = ?` | The row stops matching |
| `INSERT OR REPLACE` with every column | Replaces the row: the new text matches, the old does not |
| `UPDATE` of a subset of columns | Refused: `cannot UPDATE a subset of columns on fts5 contentless-delete table`, as [Search §5](../../docs/src/project/design/search.md) already assumes |
| Index swap: `DROP TABLE fts; ALTER TABLE fts_next RENAME TO fts` | Works; the renamed table answers queries and its shadow tables follow the rename |

`sqlite_version()` is not authorised in Durable Object SQLite (`not authorized to use function`), so the
version cannot be read from inside; nothing in the design reads it.

No fallback is taken.
