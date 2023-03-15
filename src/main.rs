use clap::{Args, Parser, Subcommand};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    value::{magic::RelativePathBuf, Dict, Map, Value},
    Figment, Metadata, Profile, Provider,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tabled::{Style, Table, Tabled};
use tera::Tera;

use squill::{Config, MigrationId};

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

#[derive(Debug, Deserialize, Serialize, Args)]
struct CliConfig {
    #[clap(long, value_parser, global = true)]
    database_url: Option<String>,

    #[clap(long, value_parser, global = true)]
    migrations_dir: Option<String>,

    #[clap(long, value_parser, global = true)]
    templates_dir: Option<String>,

    #[clap(short, long, action = clap::ArgAction::Count, default_value_t = 1)]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    enable_tracing(cli.config.verbosity);

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
        Cmd::Init => init(&config),
        Cmd::New(args) => new(&config, args),
        Cmd::Renumber(args) => renumber(&config, args),
        Cmd::Status => status(&config).await,
        Cmd::Migrate => migrate(&config).await,
        Cmd::Undo => undo(&config).await,
        Cmd::Redo => redo(&config).await,
    }
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

fn init(config: &Config) -> anyhow::Result<()> {
    let paths = squill::init(config)?;

    println!("New migration files:");
    println!();
    println!("  {}", paths.up.to_string_lossy());
    println!("  {}", paths.down.to_string_lossy());
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
struct New {
    #[clap(long, value_parser)]
    id: Option<i64>,

    #[clap(long, value_parser)]
    name: String,
}

fn new(config: &Config, args: New) -> anyhow::Result<()> {
    // TODO: chrono -> time
    let id = args.id.unwrap_or_else(|| chrono::Utc::now().timestamp());

    let paths = squill::new(config, id.try_into()?, args.name)?;

    println!("New migration files:");
    println!();
    println!("  {}", paths.up.to_string_lossy());
    println!("  {}", paths.down.to_string_lossy());
    println!();
    println!("Run `squill migrate` to apply the up migration.");

    Ok(())
}

#[derive(Args, Debug)]
struct Renumber {
    #[clap(long, value_parser, default_value = "false")]
    write: bool,
}

fn display_optional(o: &Option<impl std::fmt::Display>) -> String {
    match o {
        Some(s) => s.to_string(),
        None => "".to_string(),
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

#[derive(Debug, Clone, Tabled)]
struct Rename {
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    from: PathBuf,
    #[tabled(display_with = "std::path::Path::to_string_lossy")]
    to: PathBuf,
}

fn renumber(config: &Config, args: Renumber) -> anyhow::Result<()> {
    let renames = squill::renumber(config)?;

    if renames.is_empty() {
        return Err(anyhow::anyhow!("No migrations to renumber"));
    }

    // TODO: Skip listing unchanged names?
    let renames: Vec<Rename> = renames
        .into_iter()
        .map(|r| Rename {
            from: r.from,
            to: r.to,
        })
        .collect();

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

#[derive(Debug, Clone, Tabled)]
struct MigrationStatus {
    id: i64,
    name: String,
    #[tabled(display_with = "display_optional")]
    run_at: Option<chrono::NaiveDateTime>,
    comment: &'static str,
}

async fn status(config: &Config) -> anyhow::Result<()> {
    let status = squill::status(config).await?;

    let all_ids = {
        let mut ids: Vec<MigrationId> = Vec::new();
        ids.extend(status.applied.keys());
        ids.extend(status.available.keys());
        ids.sort();
        ids
    };

    let mut rows = Vec::new();
    for id in all_ids {
        match (status.applied.get(&id), status.available.get(&id)) {
            (Some(row), Some(_)) => {
                rows.push(MigrationStatus {
                    id: row.id.into(),
                    name: row.name.clone(),
                    run_at: Some(row.run_at),
                    comment: "",
                });
            }
            (Some(row), None) => {
                rows.push(MigrationStatus {
                    id: row.id.into(),
                    name: row.name.clone(),
                    run_at: Some(row.run_at),
                    comment: "(missing directory)",
                });
            }
            (None, Some(dir)) => {
                rows.push(MigrationStatus {
                    id: dir.id.into(),
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

async fn migrate(config: &Config) -> anyhow::Result<()> {
    let unapplied = squill::unapplied(config).await?;
    let mut conn = config.connect().await?;

    for migration in unapplied {
        println!("Running up migration: {}", migration);
        migration.up(&mut conn).await?;
    }

    Ok(())
}

async fn undo(config: &Config) -> anyhow::Result<()> {
    let mut conn = config.connect().await?;

    let migration = squill::last_applied(config, &mut conn).await?;

    println!("Running down migration: {}", migration);
    migration.down(&mut conn).await?;

    Ok(())
}

pub async fn redo(config: &Config) -> anyhow::Result<()> {
    let mut conn = config.connect().await?;

    let migration = squill::last_applied(config, &mut conn).await?;

    println!("Undoing migration: {}", migration);
    migration.down(&mut conn).await?;

    println!("Redoing migration: {}", migration);
    migration.up(&mut conn).await?;

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
