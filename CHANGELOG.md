# Changelog

Each released version names its release date, and the date its code becomes available under the
Apache License 2.0: the second anniversary of the release (FSL-1.1-ALv2, [LICENSE.md](LICENSE.md)).
`cargo xtask release --check-version` refuses a tag whose version has no entry here, or whose Apache
date is not two years after its release date. The draft GitHub Release takes its notes from the entry.

The heading of a version is `## v<version> — <release date> (Apache-2.0 on <date>)`, written when the
version is tagged.

## Unreleased

- The service, the CLI (`idvy`), the Rust SDK and the documentation as built through build plan M19.
