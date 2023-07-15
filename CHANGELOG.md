# Changelog

The format of this file is based on [Keep a Changelog].

This project uses [Semantic Versioning], and is currently in a pre-release state.

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[Keep a Changelog]: https://keepachangelog.com/en/1.0.0/

## Unreleased

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
