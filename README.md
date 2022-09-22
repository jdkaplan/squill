# Drift

Drift manages Postgresql database migrations.

There's no shortage of tools for running migrations, but this one embodies my
particular opinions:

1. Migrations should be written in database-specific SQL.
2. Migrations are not generally idempotent or reversible in production, but
   reversals (down migrations) are very useful during development.
3. Migration dependencies form a tree structure, not a linear sequence.

## Installation

To install Drift as a command, use `cargo install`:

```bash
cargo install --git https://github.com/jdkaplan/drift
```

This will be a git install until I come up with a new name. Someone already
claimed `drift` on crates.io.

To use Drift as a library, add this to your `Cargo.toml`:

```bash
drift = "0.1"
```

## Usage

Run `drift --help` to get usage information from each subcommand.

### First-time setup

Write the configuration file (`drift.toml`) or set the equivalent environment
variables. The environment variables take precedence over the file.

The environment variables are uppercase versions of the ones in the file with
`DRIFT_` prefixes. For example, `database_url` is `DRIFT_DATABASE_URL`.

```toml
# The connection string for the database to run migrations on.
#
# You might prefer to set this using an environment variable.
#
# Default: "" (default PostgreSQL server)
database_url = ""

# The directory used to store migration files.
#
# Default: "migrations"
migrations_dir = "migrations"

# The template to use for new migration files.
#
# Default: "" (use the embedded default migration templates)
templates_dir = ".drift/templates"
```

Then, generate the first migration that sets up Drift's requirements:

```bash
drift init
```

That should have written `0000000000-init/{up,down}.sql` to your migrations
directory. Read through those files and make any changes you want.

Finally, run the up migration:

```bash
drift migrate
```

### Writing a new migration

Create a new empty migration file:

```bash
drift new --name 'create_users_table'
```

Write your migration in the file. Then run it:

```bash
drift migrate
```

### Undoing a migration

For a migration that has already been run in production (or some other shared
environment), the best option is to write a new migration to undo the old one.

In a development environment, undo the most recently run migration (by
application order, not by ID):

```bash
drift undo
```

Edit the up migration, and then use `drift migrate` as normal to run it.

To make this easier, `drift redo` will run `down.sql` and then `up.sql` for the
most recently run migration.

## License

Source code and binaries are distributed under the terms of the MIT license.

## Contributing

I welcome contributions from anyone who finds a way to make this better.

In particular, I appreciate these things:
- Pull requests with clear motivation (tests are nice too!)
- Bug reports with reproducible setup instructions
- Ideas about things that work but can be improved
- Comments in issues and PRs to help me write better words and code.

## Support

This is is hobby project, so I'll only work on it as much as I find it fun to
do so. That said, I find software maintenance techniques interesting, so feel
free to start a conversation about a stable v1 if you start relying on this for
something important.
