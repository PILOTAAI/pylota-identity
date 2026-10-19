# D1 migrations

The control-plane schema ([Data model § 1](../../docs/src/project/design/data-model.md#1-d1-control-plane)),
as `NNNN_name.sql` files applied in numeric order. `pmail deploy` applies the bundle's copies through
the D1 query API and records each version in `schema_migrations`
([CLI and setup § 8.5](../../docs/src/project/design/cli.md#85-d1-migrations)); `cargo xtask itest`
applies them to its local database with `wrangler d1 migrations apply --local`.

Until v1.0 there is one migration, `0001_init.sql`, written by build plan M5. Only `.sql` files here
are migrations.
