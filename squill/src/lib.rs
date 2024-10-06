#![warn(clippy::unwrap_used)]

use lazy_static::lazy_static;
use regex::Regex;

pub mod config;
pub mod db;
pub mod index;
pub mod migrate;
pub mod status;
pub mod template;

use crate::config::{Config, ConnectError};
use crate::index::{CreateMigrationError, IndexError, IoError, MigrationIndex, MigrationParams};
use crate::migrate::{MigrateError, MigrationDirectory, MigrationId};
use crate::status::{Status, StatusError};
use crate::template::{TemplateContext, TemplateError, TemplateGroup, TemplateId, Templates};

#[cfg(test)]
mod testing;

pub async fn migrate_all(config: &Config) -> Result<Vec<MigrationDirectory>, MigrateAllError> {
    let status = Status::new(config).await.map_err(MigrateAllError::Status)?;

    let mut conn = config.connect().await.map_err(MigrateAllError::Connect)?;

    let mut applied = Vec::new();

    for migration in status.pending() {
        migration
            .up(&mut conn)
            .await
            .map_err(MigrateAllError::Migrate)?;
        applied.push(migration);
    }

    Ok(applied)
}

#[derive(thiserror::Error, Debug)]
pub enum MigrateAllError {
    #[error(transparent)]
    Status(StatusError),

    #[error(transparent)]
    Connect(ConnectError),

    #[error(transparent)]
    Migrate(MigrateError),
}

pub fn create_init_migration(config: &Config) -> Result<MigrationDirectory, NewMigrationError> {
    let templates = Templates::default();

    let mut index =
        MigrationIndex::new(&config.migrations_dir).map_err(NewMigrationError::Index)?;

    let id = MigrationId(0);
    let name = "init".to_owned();

    let ctx = TemplateContext {
        id,
        name: name.clone(),
    };

    let up_sql = templates
        .render(TemplateGroup::Default, TemplateId::InitUp, &ctx)
        .map_err(NewMigrationError::Template)?;

    let down_sql = templates
        .render(TemplateGroup::Default, TemplateId::InitDown, &ctx)
        .map_err(NewMigrationError::Template)?;

    let params = MigrationParams {
        id,
        name,
        up_sql,
        down_sql,
    };

    index.create(params).map_err(NewMigrationError::Create)
}

pub fn create_new_migration(
    config: &Config,
    template: Option<impl Into<String>>,
    id: MigrationId,
    name: impl AsRef<str>,
) -> Result<MigrationDirectory, NewMigrationError> {
    let name = name.as_ref();

    let templates = if let Some(dir) = &config.templates_dir {
        Templates::new(dir).map_err(NewMigrationError::Template)?
    } else {
        Templates::default()
    };

    let group = match template {
        Some(s) => TemplateGroup::Named(s.into()),
        None => TemplateGroup::Default,
    };

    let mut index =
        MigrationIndex::new(&config.migrations_dir).map_err(NewMigrationError::Index)?;

    let name = slugify(name);

    let ctx = TemplateContext {
        id,
        name: name.clone(),
    };

    let up_sql = templates
        .render(&group, TemplateId::NewUp, &ctx)
        .map_err(NewMigrationError::Template)?;

    let down_sql = templates
        .render(&group, TemplateId::NewDown, &ctx)
        .map_err(NewMigrationError::Template)?;

    let params = MigrationParams {
        id,
        name,
        up_sql,
        down_sql,
    };

    index.create(params).map_err(NewMigrationError::Create)
}

#[derive(thiserror::Error, Debug)]
pub enum NewMigrationError {
    #[error(transparent)]
    Index(IndexError),

    #[error(transparent)]
    Io(IoError),

    #[error(transparent)]
    Template(TemplateError),

    #[error(transparent)]
    Create(CreateMigrationError),
}

pub fn slugify(s: impl AsRef<str>) -> String {
    // Keep the character class aligned to accidental differences easier to find.
    #[rustfmt::skip]
    lazy_static! {
        static ref RE_SEP:    Regex = Regex::new(  r"[\-\s._/\\~]+"  ).expect("static pattern");
        static ref RE_PREFIX: Regex = Regex::new(r"\A[\-\s._/\\~]+"  ).expect("static pattern");
        static ref RE_SUFFIX: Regex = Regex::new(  r"[\-\s._/\\~]+\z").expect("static pattern");
    }
    let s = s.as_ref();

    let s = RE_PREFIX.replace_all(s, "");
    let s = RE_SUFFIX.replace_all(&s, "");

    let s = RE_SEP.replace_all(&s, "_");
    s.to_string()
}

#[cfg(test)]
mod tests {
    use sqlx::Executor;

    use crate::testing::*;

    use super::*;

    // A literal `None` needs a type annotation when used as `Option<impl AsRef<str>>`. This
    // "typed None" avoids that awkward turbofishing in every test.
    const NO_STR: Option<&str> = None;

    #[test]
    fn migration_slugs() {
        assert_eq!("exactly_what_i_typed", slugify("exactly_what_i_typed"));
        assert_eq!(
            "hyphens_become_underscores",
            slugify("hyphens_become_underscores")
        );
        assert_eq!(
            "compress_all_spacing",
            slugify(" compress\t  \r\n all      spacing   ")
        );
        assert_eq!(
            "no_special_characters",
            slugify(".no//special. .characters~")
        );
        assert_eq!(
            "windows_path_separators",
            slugify("windows//path\\separators")
        );
    }

    #[tokio::test]
    async fn nonexistent_migration_directory() {
        let env = TestEnv::new().await.unwrap();
        let mut config = env.config();

        // Set up our expected paths before changing the config.
        let expected_up_path = config.migrations_dir.join("nonexistent/0-init/up.sql");
        let expected_down_path = config.migrations_dir.join("nonexistent/0-init/down.sql");

        // Now configure the migrations directory to be a path that doesn't (yet) exist.
        config.migrations_dir = config.migrations_dir.join("nonexistent");

        create_init_migration(&config).unwrap();

        let up = std::fs::read_to_string(expected_up_path).unwrap();
        assert!(up.contains("create table schema_migrations"), "{up:?}");

        let down = std::fs::read_to_string(expected_down_path).unwrap();
        assert!(
            down.contains("drop table if exists schema_migrations"),
            "{down:?}"
        );
    }

    #[tokio::test]
    async fn initial_migration() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        create_init_migration(&config).unwrap();

        let up = std::fs::read_to_string(config.migrations_dir.join("0-init/up.sql")).unwrap();
        assert!(up.contains("create table schema_migrations"), "{up:?}");

        let down = std::fs::read_to_string(config.migrations_dir.join("0-init/down.sql")).unwrap();
        assert!(
            down.contains("drop table if exists schema_migrations"),
            "{down:?}"
        );
    }

    #[tokio::test]
    async fn new_migration_embedded_template() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        create_new_migration(&config, NO_STR, MigrationId(123), "create_users").unwrap();

        let up =
            std::fs::read_to_string(config.migrations_dir.join("123-create_users/up.sql")).unwrap();
        assert!(up.contains("-- TODO: Write your migration here!"), "{up:?}");

        let down = std::fs::read_to_string(config.migrations_dir.join("123-create_users/down.sql"))
            .unwrap();
        assert!(
            down.contains("-- TODO: Reverse the up migration's steps here."),
            "{down:?}"
        );
    }

    #[tokio::test]
    async fn new_migration_named_template() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        let templates_dir = config.templates_dir.as_ref().unwrap();
        std::fs::create_dir_all(templates_dir.join("create_table")).unwrap();
        std::fs::write(templates_dir.join("create_table/new.up.sql"), CUSTOM_UP).unwrap();
        std::fs::write(templates_dir.join("create_table/new.down.sql"), CUSTOM_DOWN).unwrap();

        create_new_migration(
            &config,
            Some("create_table"),
            MigrationId(123),
            "create_users",
        )
        .unwrap();

        let up =
            std::fs::read_to_string(config.migrations_dir.join("123-create_users/up.sql")).unwrap();
        assert!(
            up.contains("-- Up\n-- 123 --\n-- create_users --\n"),
            "{up:?}"
        );

        let down = std::fs::read_to_string(config.migrations_dir.join("123-create_users/down.sql"))
            .unwrap();
        assert!(
            down.contains("/*\nDown\n123\ncreate_users\n*/\n"),
            "{down:?}"
        );
    }

    #[tokio::test]
    async fn simulated_interactive_session() {
        // squill init
        let env = TestEnv::initialized().await.unwrap();
        let config = env.config();

        {
            // Use custom templates to write the migrations 😉
            //
            // It's okay if this test needs to change to support improvements to templating.
            let templates_dir = config.templates_dir.as_ref().unwrap();
            std::fs::write(templates_dir.join("new.up.sql"), CREATE_TABLE_UP).unwrap();
            std::fs::write(templates_dir.join("new.down.sql"), CREATE_TABLE_DOWN).unwrap();

            std::fs::create_dir_all(templates_dir.join("no_op")).unwrap();
            std::fs::write(templates_dir.join("no_op/new.up.sql"), CUSTOM_UP).unwrap();
            std::fs::write(templates_dir.join("no_op/new.down.sql"), CUSTOM_DOWN).unwrap();
        }

        // squill new (different from application order!)
        create_new_migration(&config, NO_STR, MigrationId(1), "users").unwrap();
        create_new_migration(&config, NO_STR, MigrationId(34567), "profiles").unwrap();
        create_new_migration(&config, NO_STR, MigrationId(200), "passwords").unwrap();
        create_new_migration(&config, Some("no_op"), MigrationId(8), "no op").unwrap();

        // squill status
        let status = Status::new(&config).await.unwrap();
        assert_eq!(4, status.pending().len());

        // squill migrate
        migrate_all(&config).await.unwrap();

        let status = Status::new(&config).await.unwrap();
        assert_eq!(0, status.pending().len());

        // Make sure all the tables exist
        let mut conn = config.connect().await.unwrap();
        for query in [
            "select * from tbl_users limit 1",
            "select * from tbl_profiles limit 1",
            "select * from tbl_passwords limit 1",
        ] {
            conn.execute(query).await.unwrap();
        }

        // squill undo
        let status = Status::new(&config).await.unwrap();
        let last = status.applied.last().unwrap();
        let last = status.available.get(last.id).unwrap();

        // Pretend that only_up was set by default.
        let mut conn = config.connect().await.unwrap();
        match last.down(&mut conn, true).await {
            Err(MigrateError::OnlyUp) => (),

            Err(err) => panic!("Unexpected error: {:?}", err),
            Ok(_) => panic!("Unexpected success"),
        }

        // Now unset only_up to allow the reversal.
        let mut conn = config.connect().await.unwrap();
        last.down(&mut conn, false).await.unwrap();

        // Make sure the right tables exist
        let mut conn = config.connect().await.unwrap();
        for query in [
            "select * from tbl_users limit 1",
            "select * from tbl_passwords limit 1",
        ] {
            conn.execute(query).await.unwrap();
        }
        for err_query in ["select * from tbl_profiles limit 1"] {
            conn.execute(err_query).await.unwrap_err();
        }
    }
}
