# Changelog

The format of this file is based on [Keep a Changelog].

This project uses [Semantic Versioning], and is currently in a pre-release state.

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[Keep a Changelog]: https://keepachangelog.com/en/1.0.0/

## Unreleased

## [0.9.2](https://github.com/jdkaplan/squill/compare/squill-v0.9.1...squill-v0.9.2) - 2024-10-06

### Other

- *(deps)* bump sqlx from 0.7.4 to 0.8.2 ([#196](https://github.com/jdkaplan/squill/pull/196))
- *(deps)* bump thiserror from 1.0.61 to 1.0.64 ([#195](https://github.com/jdkaplan/squill/pull/195))
- *(deps)* bump uuid from 1.6.1 to 1.10.0 ([#191](https://github.com/jdkaplan/squill/pull/191))
- *(deps)* bump tera from 1.19.1 to 1.20.0 ([#180](https://github.com/jdkaplan/squill/pull/180))
- *(deps)* bump sqlx from 0.7.3 to 0.7.4
- *(deps)* bump regex from 1.10.3 to 1.10.5
- *(deps)* bump tokio from 1.35.1 to 1.38.0
- *(deps)* bump thiserror from 1.0.57 to 1.0.61

## [0.9.1](https://github.com/jdkaplan/squill/compare/squill-v0.9.0...squill-v0.9.1) - 2024-06-29

### Other
- *(deps)* Remove unused dependency features
- *(deps)* bump time from 0.3.32 to 0.3.36

## [0.9.0](https://github.com/jdkaplan/squill/compare/v0.8.0...squill-v0.9.0) - 2024-06-27

### Added
- [**breaking**] Add named migration templates

### Other
- [**breaking**] Split into bin and lib packages

## [0.8.0](https://github.com/jdkaplan/squill/compare/v0.7.0...v0.8.0) - 2024-02-27

### Fixed
- Print no-migrations message on empty status ([#147](https://github.com/jdkaplan/squill/pull/147))
- Fix typo in init.up.sql comment ([#146](https://github.com/jdkaplan/squill/pull/146))

### Changed
- Switch from native-tls to rustls ([#149](https://github.com/jdkaplan/squill/pull/149))

## [0.7.0](https://github.com/jdkaplan/squill/compare/v0.6.0...v0.7.0) - 2024-02-23

### Changes
- [**breaking**] Remove deprecated aliases for align-ids ([#143](https://github.com/jdkaplan/squill/pull/143))

### Development
- Configure cargo-dist to publish binary releases

## [0.6.0](https://github.com/jdkaplan/squill/compare/v0.5.2...v0.6.0) - 2024-02-23

### Fixed
- [**breaking**] Handle long verbosity flag correctly ([#140](https://github.com/jdkaplan/squill/pull/140))

## [0.5.2](https://github.com/jdkaplan/squill/compare/v0.5.1...v0.5.2) - 2024-02-23

### Fixed

- (all commands): Print invalid directory names to the verbose log.

## 0.5.1 - 2023-07-18

### Fixes

- `migrate`: Print the count of pending migrations to avoid empty output

## 0.5.0 - 2023-07-15

### Features

- Now with modules and types!
- Derive useful standard library traits on types
- Replace `anyhow::Result` with operation-specific error enums

### Fixes

- `init`, `new`, `renumber`/`align-ids`: Allow filesystem-only commands without DB config
- `status`: Avoid printing duplicate rows for applied migrations
- `status`: Print local migration directory name if present
- `status`: Remove redundant "comment" field
- `init`, `new`: Error if migration ID already exists
- Make `-v`/`--verbosity` a global flag

### Changes

- Rename the `renumber --write` subcommand to `align-ids --execute`. The old subcommand and flag
  will be available as hidden aliases until the next set of breaking changes.

### Docs

- Update the init migration comment text to clarify what the `no-transaction` directive does.
- Add help text for all CLI subcommands and flags

### Development

- Now with tests!
- Ignore CLI files when running the command from this repo
- Add `docker-compose.yml` for local integration testing

## 0.4.2 - 2023-03-19

### Fixes

- Update `tempfile` dependency, which removes a dependency on `remove_dir_all`.
  See [GHSA-mc8h-8q98-g5hr](https://github.com/advisories/GHSA-mc8h-8q98-g5hr)

- Update `time` dependency and use it directly instead of through `chrono`.
  See [GHSA-wcg3-cvx6-7396](https://github.com/advisories/GHSA-wcg3-cvx6-7396)

## 0.4.1 - 2023-03-15

### Dev Changes

- Configure Dependabot.
- Update to `clap` v4.

## 0.4.0 - 2023-03-14

### Features

- Split into binary and library crates.

### Fixes

- Remove blank line from the end of the embedded `init.up.sql` template.

### Changes

- Change table formatting for `status` and `renumber` commands.

## 0.3.0 - 2022-10-21

Rename from "Drift" to "Squill".
Release on [crates.io](https://crates.io) as [squill](https://crates.io/crates/squill).

## 0.2.0 (Never released)

### Features

- Load custom templates for new migrations from a configurable directory.
- Use Tera for templating these migrations.

## 0.1.1 (Never released)

### Features

- Add basic commands for working with migration files: `init`, `new` `renumber`, `status`
- Add basic commands for running migrations: `migrate`, `undo`, `redo`

## 0.1.0 - (Never released)

The original version of Squill was written in Go and named "Drift". You can still find some remnants if you do some Git archaeology ;)
