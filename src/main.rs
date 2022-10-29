use clap::{Args, Parser, Subcommand};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    value::{magic::RelativePathBuf, Dict, Map, Value},
    Figment, Metadata, Profile, Provider,
};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tabled::{Style, Table, Tabled};
use tera::{Context, Tera};

// TODO: Extract parts of this into a library crate.

#[derive(Parser, Debug)]
#[clap(version)]
struct Cli {
    #[clap(subcommand)]
    command: Cmd,

    #[clap(flatten)]
    config: CliConfig,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    Init,
    New(New),
    Renumber(Renumber),
    Status,
    Migrate,
    Undo,
    Redo,
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    // TODO: don't error on missing DB URL for FS-only commands
    database_url: String,
    migrations_dir: RelativePathBuf,
    templates_dir: Option<RelativePathBuf>,
}

#[derive(Debug, Deserialize, Serialize, Args)]
struct CliConfig {
    #[clap(long, value_parser, global = true)]
    database_url: Option<String>,

    #[clap(long, value_parser, global = true)]
    migrations_dir: Option<String>,

    #[clap(long, value_parser, global = true)]
    templates_dir: Option<String>,
}

impl Provider for CliConfig {
    fn metadata(&self) -> Metadata {
        Metadata::named("command line argument(s)")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();

        if let Some(s) = &self.database_url {
            dict.insert("database_url".to_string(), Value::from(s.clone()));
        }

        if let Some(s) = &self.migrations_dir {
            dict.insert("migrations_dir".to_string(), Value::from(s.clone()));
        }

        if let Some(s) = &self.templates_dir {
            dict.insert("templates_dir".to_string(), Value::from(s.clone()));
        }

        Ok(Profile::Default.collect(dict))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config: Config = Figment::new()
        .merge(Serialized::<RelativePathBuf>::default(
            "migrations_dir",
            "migrations".into(),
        ))
        .merge(Toml::file("squill.toml"))
        .merge(Env::prefixed("SQUILL_"))
        .merge(cli.config)
        .extract()?; // TODO: improve config error reporting from here

    match cli.command {
        Cmd::Init => init(config),
        Cmd::New(args) => new(config, args),
        Cmd::Renumber(args) => renumber(config, args),
        Cmd::Status => status(config).await,
        Cmd::Migrate => migrate(config).await,
        Cmd::Undo => undo(config).await,
        Cmd::Redo => redo(config).await,
    }
}

#[derive(Args, Debug)]
struct New {
    #[clap(long, value_parser)]
    id: Option<i64>,

    #[clap(long, value_parser)]
    name: String,
}

#[derive(Args, Debug)]
struct Renumber {
    #[clap(long, value_parser, default_value = "false")]
    write: bool,
}

#[derive(Clone, Debug)]
struct Migration {
    id: MigrationId,
    name: String,
    path: PathBuf,
}

impl std::fmt::Display for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.to_string_lossy())
    }
}

impl Migration {
    async fn up(&self, conn: &mut PgConnection) -> anyhow::Result<()> {
        let path = self.path.join("up.sql");

        let sql = std::fs::read_to_string(path)?;

        if RE_NO_TX.is_match(&sql) {
            conn.execute(&*sql).await?;
        } else {
            let id = self.id.0;
            let name = self.name.clone();

            conn.transaction(|conn| {
                Box::pin(async move {
                    conn.execute(
                        sqlx::query("select _squill_claim_migration($1, $2)")
                            .bind(id)
                            .bind(name),
                    )
                    .await?;
                    conn.execute(&*sql).await
                })
            })
            .await?;
        }
        Ok(())
    }

    async fn down(&self, conn: &mut PgConnection) -> anyhow::Result<()> {
        let path = self.path.join("down.sql");

        let sql = std::fs::read_to_string(path)?;

        if RE_NO_TX.is_match(&sql) {
            conn.execute(&*sql).await?;
        } else {
            let id = self.id.0;

            conn.transaction(|conn| {
                Box::pin(async move {
                    conn.execute(sqlx::query("select _squill_unclaim_migration($1)").bind(id))
                        .await?;
                    conn.execute(&*sql).await
                })
            })
            .await?;
        }
        Ok(())
    }
}

// Migration ID has to fit in an i64 for Postgres purposes, but it should always be non-negative.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct MigrationId(i64);

impl MigrationId {
    fn width(&self) -> usize {
        // TODO(int_log): self.0.checked_log10().unwrap_or(0) + 1
        format!("{}", self.0).chars().count()
    }
}

#[derive(thiserror::Error, Debug)]
enum ParseMigrationIdError {
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("negative number: {0}")]
    Negative(i64),
}

impl std::str::FromStr for MigrationId {
    type Err = ParseMigrationIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let i: i64 = s.parse()?;
        if i < 0 {
            return Err(Self::Err::Negative(i));
        }
        Ok(Self(i))
    }
}

lazy_static! {
    static ref RE_MIGRATION: Regex = Regex::new(r"^(?P<id>\d+)-(?P<name>.*)$").unwrap();
    static ref RE_NO_TX: Regex = Regex::new("(?m)^--squill:no-transaction").unwrap();
}

fn available_migrations(dir: PathBuf) -> anyhow::Result<Vec<Migration>> {
    let mut paths: Vec<Migration> = fs::read_dir(dir)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() {
                let m = RE_MIGRATION.captures(path.file_name()?.to_str()?)?;

                let id = m.name("id")?.as_str().parse().ok()?;
                let name = m.name("name")?.as_str().to_string();

                Some(Migration { id, name, path })
            } else {
                None
            }
        })
        .collect();

    paths.sort_by_key(|m| m.id.0);
    Ok(paths)
}

#[derive(sqlx::FromRow)]
struct MigrationRow {
    id: i64,
    name: String,
    run_at: chrono::NaiveDateTime,
}

async fn applied_migrations(conn: &mut PgConnection) -> anyhow::Result<Vec<MigrationRow>> {
    let query = sqlx::query_as("select * from schema_migrations order by id asc");
    match query.fetch_all(conn).await {
        Ok(res) => Ok(res),
        Err(err) => {
            if let sqlx::Error::Database(ref db_err) = err {
                if let Some(code) = db_err.code() {
                    // undefined_table
                    if code == "42P01" {
                        // The expected table doesn't exist. This is probably because we haven't
                        // run the first migration that will create this table.
                        return Ok(Vec::new());
                    }
                }
            }
            Err(err.into())
        }
    }
}

#[derive(Debug, Clone, Tabled)]
struct MigrationStatus {
    id: i64,
    name: String,
    #[tabled(display_with = "display_optional")]
    run_at: Option<chrono::NaiveDateTime>,
    comment: &'static str,
}

fn display_optional(o: &Option<impl std::fmt::Display>) -> String {
    match o {
        Some(s) => s.to_string(),
        None => "".to_string(),
    }
}

async fn status(config: Config) -> anyhow::Result<()> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    // TODO: There's definitely a more efficient way to do this, but 🤷

    let applied: HashMap<MigrationId, MigrationRow> = applied_migrations(&mut conn)
        .await?
        .into_iter()
        .map(|row| (MigrationId(row.id), row))
        .collect();

    let available: HashMap<MigrationId, Migration> =
        available_migrations(config.migrations_dir.relative())?
            .into_iter()
            .map(|m| (m.id, m))
            .collect();

    let applied_ids: HashSet<MigrationId> = applied.keys().cloned().collect();
    let available_ids: HashSet<MigrationId> = available.keys().cloned().collect();
    let mut all_ids: Vec<MigrationId> = applied_ids.union(&available_ids).cloned().collect();

    all_ids.sort();

    let mut rows = Vec::new();
    for id in all_ids {
        match (applied.get(&id), available.get(&id)) {
            (Some(row), Some(_)) => {
                rows.push(MigrationStatus {
                    id: row.id,
                    name: row.name.clone(),
                    run_at: Some(row.run_at),
                    comment: "",
                });
            }
            (Some(row), None) => {
                rows.push(MigrationStatus {
                    id: row.id,
                    name: row.name.clone(),
                    run_at: Some(row.run_at),
                    comment: "(missing directory)",
                });
            }
            (None, Some(dir)) => {
                rows.push(MigrationStatus {
                    id: dir.id.0,
                    name: dir.name.clone(),
                    run_at: None,
                    comment: "todo",
                });
            }
            (None, None) => (), // This is impossible, right?
        }
    }

    print_table(rows);
    Ok(())
}

async fn migrate(config: Config) -> anyhow::Result<()> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    let applied: HashMap<MigrationId, MigrationRow> = applied_migrations(&mut conn)
        .await?
        .into_iter()
        .map(|row| (MigrationId(row.id), row))
        .collect();

    for migration in available_migrations(config.migrations_dir.relative())? {
        if applied.contains_key(&migration.id) {
            continue;
        }

        println!("Running up migration: {}", migration);
        migration.up(&mut conn).await?;
    }

    Ok(())
}

async fn undo(config: Config) -> anyhow::Result<()> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    let migration = last_applied(config, &mut conn).await?;

    println!("Running down migration: {}", migration);
    migration.down(&mut conn).await?;

    Ok(())
}

async fn redo(config: Config) -> anyhow::Result<()> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    let migration = last_applied(config, &mut conn).await?;

    println!("Undoing migration: {}", migration);
    migration.clone().down(&mut conn).await?;

    println!("Redoing migration: {}", migration);
    migration.up(&mut conn).await?;

    Ok(())
}

async fn last_applied(config: Config, conn: &mut PgConnection) -> anyhow::Result<Migration> {
    let applied = applied_migrations(conn).await?;

    let last = match applied.iter().max_by_key(|row| row.run_at) {
        None => return Err(anyhow::anyhow!("No migrations have been run.")),
        Some(last) => last,
    };

    let matches: Vec<Migration> = available_migrations(config.migrations_dir.relative())?
        .iter()
        .cloned()
        .filter(|m| m.id.0 == last.id)
        .collect();

    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(anyhow::anyhow!(
            "No migration directory found for migration ID: {}",
            last.id
        )),
        n => Err(anyhow::anyhow!(
            "{} migration directories found for migration ID: {}",
            n,
            last.id
        )),
    }
}

// These migration files either have no parameters (init) or will be modified before being run
// (new). The arguments to new migrations come from the same person who will be making those
// changes.
//
// So although configuring SQL escaping would be nice, I'm not worried about it for now.
//
// TODO: Call Tera::set_escape_fn to make me feel better.

lazy_static! {
    static ref TERA: Tera = {
        let mut tera = Tera::default();
        tera.add_raw_template("init.up.sql", include_str!("templates/init.up.sql"))
            .unwrap();
        tera.add_raw_template("init.down.sql", include_str!("templates/init.down.sql"))
            .unwrap();
        tera.add_raw_template("new.up.sql", include_str!("templates/new.up.sql"))
            .unwrap();
        tera.add_raw_template("new.down.sql", include_str!("templates/new.down.sql"))
            .unwrap();
        tera
    };
}

fn load_templates(templates_dir: RelativePathBuf) -> anyhow::Result<Tera> {
    let mut tera = Tera::default();

    let up_path = templates_dir.relative().join("new.up.sql");
    let up_content = std::fs::read_to_string(up_path)?;
    tera.add_raw_template("new.up.sql", &up_content)?;

    let down_path = templates_dir.relative().join("new.down.sql");
    let down_content = std::fs::read_to_string(down_path)?;
    tera.add_raw_template("new.down.sql", &down_content)?;

    Ok(tera)
}

fn init(config: Config) -> anyhow::Result<()> {
    let id = 0;
    let name = "init";

    let dir = config
        .migrations_dir
        .relative()
        .join(format!("{}-{}", id, name));

    let ctx = {
        let mut ctx = Context::new();
        ctx.insert("id", &id);
        ctx.insert("name", name);
        ctx
    };

    let up_content = TERA.render("init.up.sql", &ctx)?;
    let down_content = TERA.render("init.down.sql", &ctx)?;

    create_migration(dir, up_content.as_bytes(), down_content.as_bytes())?;

    println!("Run the `migrate` subcommand to apply this migration.");

    Ok(())
}

fn new(config: Config, args: New) -> anyhow::Result<()> {
    let id = match args.id {
        Some(id) => id,
        None => chrono::Utc::now().timestamp(),
    };

    let name = slugify(args.name);

    let dir = config
        .migrations_dir
        .relative()
        .join(format!("{}-{}", id, name));

    let ctx = {
        let mut ctx = Context::new();
        ctx.insert("id", &id);
        ctx.insert("name", &name);
        ctx
    };

    let (up_content, down_content) = match config.templates_dir {
        Some(dir) => {
            let tera = load_templates(dir)?;
            let up = tera.render("new.up.sql", &ctx)?;
            let down = tera.render("new.down.sql", &ctx)?;
            (up, down)
        }
        None => {
            let up = TERA.render("new.up.sql", &ctx)?;
            let down = TERA.render("new.down.sql", &ctx)?;
            (up, down)
        }
    };

    create_migration(dir, up_content.as_bytes(), down_content.as_bytes())?;

    println!("Run the `migrate` subcommand to apply this migration.");

    Ok(())
}

fn slugify(s: String) -> String {
    lazy_static! {
        static ref RE_SEP: Regex = Regex::new(r"[\-\s._/]+").unwrap();
    }
    RE_SEP.replace_all(&s, "_").to_string()
}

fn create_migration(dir: PathBuf, up_sql: &[u8], down_sql: &[u8]) -> anyhow::Result<()> {
    let up_path = dir.join("up.sql");
    let down_path = dir.join("down.sql");

    println!("Creating migration directory: {}", dir.to_string_lossy());
    fs::create_dir_all(&dir)?;

    println!("Creating up migration file: {}", up_path.to_string_lossy());
    fs::File::create(&up_path)?.write_all(up_sql)?;

    println!(
        "Creating down migration file: {}",
        down_path.to_string_lossy()
    );
    fs::File::create(&down_path)?.write_all(down_sql)?;

    Ok(())
}

#[derive(Debug, Clone, Tabled)]
struct Rename {
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    from: PathBuf,
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    to: PathBuf,
}

fn renumber(config: Config, args: Renumber) -> anyhow::Result<()> {
    let migrations = available_migrations(config.migrations_dir.relative())?;

    if migrations.is_empty() {
        return Err(anyhow::anyhow!("No migrations to renumber"));
    }

    let width = migrations.iter().map(|m| m.id.width()).max().unwrap();

    let mut renames = Vec::new();
    for m in migrations {
        let old = m.path.clone();

        let new = m
            .path
            .with_file_name(format!("{:0width$}-{}", m.id.0, m.name));

        renames.push(Rename { from: old, to: new });
    }

    print_table(&renames);
    println!();

    if args.write {
        print!("Renaming files...");
        for r in renames {
            fs::rename(r.from, r.to)?;
        }
        println!(" done!");
    } else {
        println!("Skipping the actual renames because writes were not enabled.");
        println!("Add --write to do the rename.");
    }

    Ok(())
}

fn print_table<I, T>(rows: I)
where
    I: IntoIterator<Item = T>,
    T: Tabled,
{
    let mut table = Table::new(rows);
    table.with(Style::sharp());
    println!("{}", table);
}
