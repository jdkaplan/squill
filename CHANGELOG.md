# Changelog

The format of this file is based on [Keep a Changelog].

This project uses [Semantic Versioning], and is currently in a pre-release state.

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[Keep a Changelog]: https://keepachangelog.com/en/1.0.0/

## Unreleased

## 0.4.1 - 2023-03-15

### Dev Changes

- Configure Dependabot.
- Update to clap v4.

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

**Full Changelog**: https://github.com/jdkaplan/squill/commits/v0.3.0

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
