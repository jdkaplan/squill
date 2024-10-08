# Squill

Squill manages Postgresql database migrations.

There's no shortage of tools for running migrations, but this one embodies my
particular opinions:

1. Migrations should be written in database-specific SQL.
2. Migrations are not generally idempotent or reversible in production, but
   reversals (down migrations) are very useful during development.
3. Migration dependencies form a tree structure, not a linear sequence.

## What's a squill?

It's the common name for a subfamily of plants that are actually pretty cool looking.

But more importantly, it's a word that has the letters "s", "q", and "l" in
that order, is easy to type and pronounce, and not already someone else's crate
name 😉

## Installation

To install Squill as a command, use `cargo install`:

```bash
cargo install squill-cli
```

or download a pre-built package from the [GitHub Releases].

[GitHub Releases]: https://github.com/jdkaplan/squill/releases?q=squill-cli

To use Squill as a library, use `cargo add`:

```bash
cargo add squill
```

## Usage

Run `squill --help` to get usage information from each subcommand.

### First-time setup

Write the configuration file (`squill.toml`) or set the equivalent environment
variables. The environment variables take precedence over the file.

The environment variables are uppercase versions of the ones in the file with
`SQUILL_` prefixes. For example, `database_url` is `SQUILL_DATABASE_URL`.

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
# Default: (unset) (use the embedded default migration templates)
templates_dir = ".squill/templates"

# Whether only up migrations should be allowed. This can be used to avoid
# accidental data loss in shared environments.
#
# Default: false (allow down migrations)
only_up = true
```

Then, generate the first migration that sets up Squill's requirements:

```bash
squill init
```

That should have written `0-init/{up,down}.sql` to your migrations directory.
Read through those files and make any changes you want.

Finally, run the up migration:

```bash
squill migrate
```

### Writing a new migration

Create a new empty migration file:

```bash
squill new --name 'create_users_table'
```

(You can override the automatic ID generation with `--id 123`).

Write your migration in the file. Then run it:

```bash
squill migrate
```

### Undoing a migration

For a migration that has already been run in production (or some other shared
environment), the best option is to write a new migration to undo the old one.

In a development environment, undo the most recently run migration (by
application order, not by ID):

```bash
squill undo
```

Edit the up migration, and then use `squill migrate` as normal to run it.

To make this easier, `squill redo` will run `down.sql` and then `up.sql` for the
most recently run migration.

### Renumbering migrations

You may have a mix of migrations with different ID lengths, which can make it
the directory listing appear out of order. Use the `align-ids` subcommand to
zero-pad shorter IDs:

```bash
squill align-ids
```

That command is just a preview by default. Add `--execute` to actually execute
all of the proposed renames.

### Custom migration templates

You can customize the files generated by `squill new` by setting the
`templates_dir` path. Squill will use the `new.up.sql` and `new.down.sql` files
in that directory.

```
.squill/templates
├── new.down.sql
└── new.up.sql
```

These are interpreted as [Tera] templates for generating the respective up and
down migration files.

[Tera]: https://tera.netlify.app/

The Tera context will be something like this:
```
id: &i64
name: &str
```

#### Named templates

You can keep a named migration template by making a subdirectory within
`templates_dir` and adding `new.up.sql` and `new.down.sql` inside it.

To use a named template, add the `--template` argument to the `squill new`
command. The default (unnamed) template inside `templates_dir` will be used
otherwise.

For example, if you want to ensure that `create table` migrations follow
specific conventions, your `templates_dir` could look like this:

```
.squill/templates
├── create_table
│   ├── new.down.sql
│   └── new.up.sql
├── new.down.sql
└── new.up.sql
```

And this is the command to generate a new migration that uses it:

```bash
squill new --template 'create_table' --name 'create_users_table'
```

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## Join me!

I welcome contributions from anyone who finds a way to make this better.

In particular, I appreciate these things:
- Pull requests with clear motivation (tests are nice too!)
- Bug reports with reproducible setup instructions
- Ideas about things that work but can be improved
- Comments in issues and PRs to help me write better words and code

## Support

This is is hobby project, so I'll only work on it as much as I find it fun to
do so. That said, I find software maintenance techniques interesting, so feel
free to start a conversation about a stable v1 if you start relying on this for
something important.
