use sqlx::{postgres::PgConnectOptions, ConnectOptions, Executor};
use tempfile::TempDir;
use uuid::Uuid;

use crate::index::MigrationParams;
use crate::{create_init_migration, Config};

pub const NO_OP_NO_TX: &str = include_str!("testing/no_op_no_tx.sql");
pub const NO_OP_YES_TX: &str = include_str!("testing/no_op_yes_tx.sql");
pub const CUSTOM_UP: &str = include_str!("testing/custom.up.sql");
pub const CUSTOM_DOWN: &str = include_str!("testing/custom.down.sql");
pub const CREATE_TABLE_UP: &str = include_str!("testing/create_table.up.sql");
pub const CREATE_TABLE_DOWN: &str = include_str!("testing/create_table.down.sql");

#[derive(Debug)]
pub struct TestEnv {
    pub database: TempDb,
    pub migrations_dir: TempDir,
    pub templates_dir: TempDir,
}

impl TestEnv {
    pub async fn new() -> anyhow::Result<Self> {
        let opts = PgConnectOptions::new();

        Ok(Self {
            database: TempDb::new(opts).await?,
            migrations_dir: tempfile::Builder::new().prefix("migrations_").tempdir()?,
            templates_dir: tempfile::Builder::new().prefix("templates_").tempdir()?,
        })
    }

    pub async fn initialized() -> anyhow::Result<Self> {
        let env = Self::new().await?;
        let config = env.config();

        let init = create_init_migration(&config)?;

        let mut conn = config.connect().await?;
        init.up(&mut conn).await?;

        Ok(env)
    }

    pub fn config(&self) -> Config {
        Config {
            database_connect_options: Some(self.database.connect_options.clone()),
            migrations_dir: self.migrations_dir.path().into(),
            templates_dir: Some(self.templates_dir.path().into()),
            only_up: true,
        }
    }
}

#[derive(Debug)]
pub struct TempDb {
    pub connect_options: PgConnectOptions,
}

impl TempDb {
    pub async fn new(opts: PgConnectOptions) -> anyhow::Result<Self> {
        // This name is controlled by this test, so the executed `create database`
        // statement is okay to execute.
        //
        // This has to be done with string interpolation because Postgres doesn't support
        // using a prepared statement to create a database.
        let name = format!("squill_test_{}", Uuid::new_v4().simple());
        let create_database = format!("create database {}", name);

        let mut conn = opts.connect().await?;
        conn.execute(&*create_database).await?;

        // Now that the target database has actually been created, future connections can use it.
        let opts = opts.database(&name);

        Ok(Self {
            connect_options: opts,
        })
    }
}

pub fn fake_migration(id: i64, name: &str) -> MigrationParams {
    MigrationParams {
        id: id.try_into().unwrap(),
        name: name.into(),
        up_sql: format!("create table tbl_{name} (id_{id} int)"),
        down_sql: format!("drop table tbl_{name}"),
    }
}
