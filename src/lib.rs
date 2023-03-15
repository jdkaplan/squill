use figment::value::magic::RelativePathBuf;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tera::{Context, Tera};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    // TODO: don't error on missing DB URL for FS-only commands
    pub database_url: String,
    pub migrations_dir: RelativePathBuf,
    pub templates_dir: Option<RelativePathBuf>,
}

impl Config {
    pub async fn connect(&self) -> sqlx::Result<PgConnection> {
        PgConnection::connect(&self.database_url).await
    }
}

#[derive(Clone, Debug)]
pub struct Migration {
    pub id: MigrationId,
    pub name: String,
    pub path: PathBuf,
}

impl std::fmt::Display for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.to_string_lossy())
    }
}

impl Migration {
    pub async fn up(&self, conn: &mut PgConnection) -> anyhow::Result<()> {
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

    // TODO: Add some sort of "forward-only" flag that prevents down migrations.
    pub async fn down(&self, conn: &mut PgConnection) -> anyhow::Result<()> {
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
pub struct MigrationId(i64);

impl MigrationId {
    pub fn as_i64(&self) -> i64 {
        self.0
    }
}

impl MigrationId {
    fn width(&self) -> usize {
        // Assuming the i64 is non-negative, the only edge case is zero, which can be treated
        // like other single-digit numbers that have a log10 of 0.
        let digits = 1 + self.0.checked_ilog10().unwrap_or(0);
        digits.try_into().expect("ilog10(i64) is a small number")
    }
}

impl From<MigrationId> for i64 {
    fn from(value: MigrationId) -> Self {
        value.as_i64()
    }
}

impl TryFrom<i64> for MigrationId {
    type Error = ParseMigrationIdError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value < 0 {
            return Err(Self::Error::Negative(value));
        }
        Ok(Self(value))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseMigrationIdError {
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("negative number: {0}")]
    Negative(i64),
}

impl std::str::FromStr for MigrationId {
    type Err = ParseMigrationIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let i: i64 = s.parse()?;
        Self::try_from(i)
    }
}

lazy_static! {
    static ref RE_MIGRATION: Regex = Regex::new(r"^(?P<id>\d+)-(?P<name>.*)$").unwrap();
    static ref RE_NO_TX: Regex = Regex::new("(?m)^--squill:no-transaction").unwrap();
}

pub fn available_migrations(dir: PathBuf) -> anyhow::Result<Vec<Migration>> {
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

#[derive(Debug, Clone)]
pub struct Status {
    pub applied: HashMap<MigrationId, AppliedMigration>,
    pub available: HashMap<MigrationId, Migration>,
}

#[derive(Debug, Clone)]
pub struct AppliedMigration {
    pub id: MigrationId,
    pub name: String,
    pub run_at: chrono::NaiveDateTime,
}

pub async fn status(config: &Config) -> anyhow::Result<Status> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    let applied: HashMap<MigrationId, AppliedMigration> = applied_migrations(&mut conn)
        .await?
        .into_iter()
        .map(|row| {
            (
                MigrationId(row.id),
                AppliedMigration {
                    id: MigrationId(row.id),
                    name: row.name,
                    run_at: row.run_at,
                },
            )
        })
        .collect();

    let available: HashMap<MigrationId, Migration> =
        available_migrations(config.migrations_dir.relative())?
            .into_iter()
            .map(|m| {
                (
                    m.id,
                    Migration {
                        id: m.id,
                        name: m.name,
                        path: m.path,
                    },
                )
            })
            .collect();

    Ok(Status { applied, available })
}

pub async fn unapplied(config: &Config) -> anyhow::Result<Vec<Migration>> {
    let mut conn = PgConnection::connect(&config.database_url).await?;

    let applied: HashMap<MigrationId, MigrationRow> = applied_migrations(&mut conn)
        .await?
        .into_iter()
        .map(|row| (MigrationId(row.id), row))
        .collect();

    let available = available_migrations(config.migrations_dir.relative())?;

    let unapplied: Vec<Migration> = available
        .into_iter()
        .filter_map(|m| {
            if applied.contains_key(&m.id) {
                None
            } else {
                Some(Migration {
                    id: m.id,
                    name: m.name,
                    path: m.path,
                })
            }
        })
        .collect();

    Ok(unapplied)
}

pub async fn last_applied(config: &Config, conn: &mut PgConnection) -> anyhow::Result<Migration> {
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

fn load_templates(templates_dir: &RelativePathBuf) -> anyhow::Result<Tera> {
    let mut tera = Tera::default();

    let up_path = templates_dir.relative().join("new.up.sql");
    let up_content = std::fs::read_to_string(up_path)?;
    tera.add_raw_template("new.up.sql", &up_content)?;

    let down_path = templates_dir.relative().join("new.down.sql");
    let down_content = std::fs::read_to_string(down_path)?;
    tera.add_raw_template("new.down.sql", &down_content)?;

    Ok(tera)
}

pub fn init(config: &Config) -> anyhow::Result<MigrationPaths> {
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

    create_migration(dir, up_content.as_bytes(), down_content.as_bytes())
}

pub fn new(config: &Config, id: MigrationId, name: String) -> anyhow::Result<MigrationPaths> {
    let name = slugify(name);

    let dir = config
        .migrations_dir
        .relative()
        .join(format!("{}-{}", id.as_i64(), name));

    let ctx = {
        let mut ctx = Context::new();
        ctx.insert("id", &id.as_i64());
        ctx.insert("name", &name);
        ctx
    };

    let (up_content, down_content) = match &config.templates_dir {
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

    create_migration(dir, up_content.as_bytes(), down_content.as_bytes())
}

pub fn slugify(s: String) -> String {
    lazy_static! {
        static ref RE_SEP: Regex = Regex::new(r"[\-\s._/]+").unwrap();
    }
    RE_SEP.replace_all(&s, "_").to_string()
}

#[derive(Debug, Clone)]
pub struct MigrationPaths {
    pub dir: PathBuf,
    pub up: PathBuf,
    pub down: PathBuf,
}

fn create_migration(
    dir: PathBuf,
    up_sql: &[u8],
    down_sql: &[u8],
) -> anyhow::Result<MigrationPaths> {
    let up_path = dir.join("up.sql");
    let down_path = dir.join("down.sql");

    tracing::info!("Creating migration directory: {}", dir.to_string_lossy());
    fs::create_dir_all(&dir)?;

    tracing::info!("Creating up migration file: {}", up_path.to_string_lossy());
    fs::File::create(&up_path)?.write_all(up_sql)?;

    tracing::info!(
        "Creating down migration file: {}",
        down_path.to_string_lossy()
    );
    fs::File::create(&down_path)?.write_all(down_sql)?;

    Ok(MigrationPaths {
        dir,
        up: up_path,
        down: down_path,
    })
}

#[derive(Debug, Clone)]
pub struct Rename {
    pub from: PathBuf,
    pub to: PathBuf,
}

pub fn renumber(config: &Config) -> anyhow::Result<Vec<Rename>> {
    let migrations = available_migrations(config.migrations_dir.relative())?;

    let width = migrations.iter().map(|m| m.id.width()).max().unwrap();

    let mut renames = Vec::new();
    for m in migrations {
        let old = m.path.clone();

        let new = m
            .path
            .with_file_name(format!("{:0width$}-{}", m.id.0, m.name));

        renames.push(Rename { from: old, to: new });
    }

    Ok(renames)
}
