use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use clap::{Args, Parser, Subcommand};
use figment::providers::{Env, Format, Serialized, Toml};
use figment::value::{magic::RelativePathBuf, Dict, Map, Value};
use figment::{Figment, Metadata, Profile, Provider};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgConnectOptions;
use tabled::{settings::Style, Table, Tabled};
use tokio::task::spawn_blocking;

use squill::{config::Config, index::MigrationIndex, status::Status};
use squill::{create_init_migration, create_new_migration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    enable_tracing(cli.config.verbosity);

    let fig = Figment::new()
        .merge(Serialized::<RelativePathBuf>::default(
            "migrations_dir",
            "migrations".into(),
        ))
        .merge(Toml::file("squill.toml"))
        .merge(Env::prefixed("SQUILL_"))
        .merge(cli.config);

    let config = extract(fig)?;

    cli.command.execute(config).await
}

fn enable_tracing(verbosity: u8) {
    use tracing_subscriber::filter::LevelFilter;

    let max_level = match verbosity {
        0 => LevelFilter::OFF,
        1 => LevelFilter::ERROR,
        2 => LevelFilter::INFO,
        3.. => LevelFilter::DEBUG,
    };

    tracing_subscriber::fmt()
        .pretty()
        .with_max_level(max_level)
        .init();
}

#[derive(Parser, Debug)]
#[clap(version)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Cmd,

    #[clap(flatten)]
    pub config: CliConfig,
}

#[derive(Debug, Deserialize, Serialize, Args)]
pub struct CliConfig {
    #[clap(long, value_parser, global = true)]
    database_url: Option<String>,

    #[clap(long, value_parser, global = true)]
    migrations_dir: Option<String>,

    #[clap(long, value_parser, global = true)]
    templates_dir: Option<String>,

    #[clap(short, long, action = clap::ArgAction::Count, default_value_t = 1, global=true)]
    verbosity: u8,
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

fn extract(fig: Figment) -> anyhow::Result<Config> {
    let migrations_dir: RelativePathBuf = fig.extract_inner("migrations_dir")?;

    // The templates dir is optional. If it is not set, this will use the default embedded
    // templates. This can still fail if the directory that _was_ set is invalid.
    let templates_dir: Option<RelativePathBuf> = extract_inner_or_default(&fig, "templates_dir")?;

    // Although it might not seem like it, this is easier than deriving Deserialize for a newtype
    // around PgConnectOptions.
    let database_url: Option<String> = extract_inner_or_default(&fig, "database_url")?;

    let database_connect_options = if let Some(url) = database_url {
        Some(url.parse::<PgConnectOptions>()?)
    } else {
        None
    };

    Ok(Config {
        database_connect_options,
        migrations_dir: migrations_dir.relative(),
        templates_dir: templates_dir.map(|dir| dir.relative()),
    })
}

fn extract_inner_or_default<'a, T>(fig: &Figment, key: &str) -> Result<T, figment::Error>
where
    T: Default + Deserialize<'a>,
{
    match fig.extract_inner::<T>(key) {
        Ok(val) => Ok(val),
        Err(err) => {
            for e in err.clone() {
                if e.missing() {
                    return Ok(T::default());
                }
            }
            Err(err)
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    Init,
    New(New),
    Renumber(Renumber),
    Status,
    Migrate,
    Undo,
    Redo,
}

impl Cmd {
    pub async fn execute(self, config: Config) -> anyhow::Result<()> {
        match self {
            Cmd::Init => spawn_blocking(move || init(&config)).await?,
            Cmd::New(args) => spawn_blocking(move || new(&config, args)).await?,
            Cmd::Renumber(args) => spawn_blocking(move || renumber(&config, args)).await?,

            Cmd::Status => status(&config).await,
            Cmd::Migrate => migrate(&config).await,
            Cmd::Undo => undo(&config).await,
            Cmd::Redo => redo(&config).await,
        }
    }
}

fn init(config: &Config) -> anyhow::Result<()> {
    let files = create_init_migration(config)?;

    println!("New migration files:");
    println!();
    println!("  {}", files.up_path.to_string_lossy());
    println!("  {}", files.down_path.to_string_lossy());
    println!();
    println!("This prepares the database so Squill can track which migrations have been applied.");
    println!("You can edit these files if you want to.");
    println!();
    println!("Run `squill migrate` to apply the up migration.");
    println!();
    println!("Run `squill new` to create a new migration directory.");

    Ok(())
}

#[derive(Args, Debug)]
pub struct New {
    #[clap(long, value_parser)]
    pub id: Option<i64>,

    #[clap(long, value_parser)]
    pub name: String,
}

fn new(config: &Config, args: New) -> anyhow::Result<()> {
    let id = args.id.unwrap_or_else(|| {
        let epoch_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is not before 1970");

        epoch_time
            .as_secs()
            .try_into()
            .expect("system clock is not in the far future")
    });

    let files = create_new_migration(config, id.try_into()?, args.name)?;

    println!("New migration files:");
    println!();
    println!("  {}", files.up_path.to_string_lossy());
    println!("  {}", files.down_path.to_string_lossy());
    println!();
    println!("Edit `up.sql` to perform the change you want and `down.sql` to reverse it.");
    println!();
    println!("Run `squill migrate` to apply the up migration.");

    Ok(())
}

#[derive(Args, Debug)]
pub struct Renumber {
    #[clap(long, value_parser, default_value = "false")]
    pub write: bool,
}

#[derive(Debug, Clone, Tabled)]
struct Rename {
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    from: PathBuf,
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    to: PathBuf,
}

fn renumber(config: &Config, args: Renumber) -> anyhow::Result<()> {
    let migrations = MigrationIndex::new(&config.migrations_dir)?;

    let renames = migrations.align_ids();

    if renames.is_empty() {
        return Err(anyhow::anyhow!("No migrations to rename"));
    }

    let renames: Vec<Rename> = renames
        .into_iter()
        .filter(|r| r.from != r.to)
        .map(|r| Rename {
            from: r.from,
            to: r.to,
        })
        .collect();

    if renames.is_empty() {
        println!("All migration IDs are already the same width");
        return Ok(());
    }

    print_table(&renames);
    println!();

    if args.write {
        print!("Renaming files...");
        for r in renames {
            std::fs::rename(r.from, r.to)?;
        }
        println!(" done!");
    } else {
        println!("Skipping the actual renames because writes were not enabled.");
        println!("Add --write to do the rename.");
    }

    Ok(())
}

#[derive(Debug, Clone, Tabled)]
struct MigrationStatus {
    id: i64,
    name: String,
    #[tabled(display_with = "display_optional")]
    run_at: Option<time::PrimitiveDateTime>,
    #[tabled(display_with = "display_optional")]
    directory: Option<String>,
}

async fn status(config: &Config) -> anyhow::Result<()> {
    let status = Status::new(config).await?;

    let zipped = status.full_status();

    let rows = zipped.values().cloned().map(|v| MigrationStatus {
        id: v.id.into(),
        name: v.name,
        run_at: v.run_at,
        directory: v.directory,
    });

    print_table(rows);
    Ok(())
}

// TODO: Optionally up through certain ID
async fn migrate(config: &Config) -> anyhow::Result<()> {
    let status = Status::new(config).await?;

    let mut conn = config.connect().await?;

    for migration in status.pending() {
        println!("Running up migration: {}", migration);
        migration.up(&mut conn).await?;
    }

    Ok(())
}

// TODO: Optionally _down_ to (but not below) a certain ID?

// TODO: Optionally undo a specific ID
async fn undo(config: &Config) -> anyhow::Result<()> {
    let status = Status::new(config).await?;

    let Some(migration) = status.applied.last() else {
        return Err(anyhow!("No migration to undo"));
    };

    let Some(migration) = status.available.get(migration.id) else {
        return Err(anyhow!("Could not find files for migration ID {} ({})", migration.id, migration.name))
    };

    let mut conn = config.connect().await?;

    println!("Running down migration: {}", migration);
    migration.down(&mut conn).await?;

    Ok(())
}

// TODO: Optionally redo a specific ID
pub async fn redo(config: &Config) -> anyhow::Result<()> {
    let status = Status::new(config).await?;

    let Some(migration) = status.applied.last() else {
        return Err(anyhow!("No migration to redo"));
    };

    let Some(migration) = status.available.get(migration.id) else {
        return Err(anyhow!("Could not find files for migration ID {} ({})", migration.id, migration.name))
    };

    let mut conn = config.connect().await?;

    println!("Running down migration: {}", migration);
    migration.down(&mut conn).await?;

    println!("Running up migration: {}", migration);
    migration.up(&mut conn).await?;

    Ok(())
}

fn display_optional(o: &Option<impl std::fmt::Display>) -> String {
    match o {
        Some(s) => s.to_string(),
        None => "".to_string(),
    }
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
