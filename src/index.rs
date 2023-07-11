use lazy_static::lazy_static;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{MigrationDirectory, MigrationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationIndex {
    pub(crate) dir: PathBuf,
    pub(crate) index: BTreeMap<MigrationId, MigrationDirectory>,
}

impl MigrationIndex {
    pub fn new(migrations_dir: &Path) -> Result<Self, IndexError> {
        let available = available_migrations(migrations_dir)?;

        let mut multi_index: BTreeMap<MigrationId, Vec<MigrationDirectory>> = BTreeMap::new();
        for m in available {
            multi_index.entry(m.id).or_insert(Vec::new()).push(m);
        }

        let mut index = BTreeMap::new();
        let mut multiples = BTreeMap::new();

        for (id, mut migrations) in multi_index {
            if migrations.len() == 1 {
                index.insert(id, migrations.swap_remove(0));
            } else {
                multiples.insert(id, migrations);
            }
        }

        if multiples.is_empty() {
            Ok(Self {
                dir: migrations_dir.to_path_buf(),
                index,
            })
        } else {
            Err(IndexError::MultipleMigrationDirectories(multiples))
        }
    }

    pub fn get(&self, id: MigrationId) -> Option<&MigrationDirectory> {
        self.index.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &MigrationDirectory> {
        self.index.values()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum IndexError {
    #[error("failed to read directory: {path}: {err}")]
    ReadDir { path: PathBuf, err: std::io::Error },

    #[error("multiple directories found for some migration IDs: (count={})", .0.len())]
    MultipleMigrationDirectories(BTreeMap<MigrationId, Vec<MigrationDirectory>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationParams {
    pub id: MigrationId,
    pub name: String,
    pub up_sql: String,
    pub down_sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MigrationFiles {
    pub up: PathBuf,
    pub down: PathBuf,
}

impl MigrationIndex {
    pub fn create(
        &mut self,
        params: MigrationParams,
    ) -> Result<MigrationDirectory, CreateMigrationError> {
        if let Some(migration) = self.index.get(&params.id) {
            return Err(CreateMigrationError::ExistingDirectory(migration.clone()));
        }

        let dir = self.dir.join(format!("{}-{}", params.id, params.name));

        let files = create_migration_files(&dir, params.up_sql, params.down_sql)
            .map_err(CreateMigrationError::Io)?;

        let migration = MigrationDirectory {
            id: params.id,
            name: params.name,
            dir,
            up_path: files.up,
            down_path: files.down,
        };

        self.index.insert(params.id, migration.clone());

        Ok(migration)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CreateMigrationError {
    #[error(transparent)]
    Io(IoError),

    #[error("directory already exists for migration ID: {}", .0.dir.to_string_lossy())]
    ExistingDirectory(MigrationDirectory),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rename {
    pub from: PathBuf,
    pub to: PathBuf,
}

impl MigrationIndex {
    pub fn align_ids(&self) -> Vec<Rename> {
        let width = self.iter().map(|m| m.id.width()).max().unwrap_or(10);

        let mut renames = Vec::new();
        for m in self.iter() {
            let old = m.dir.clone();

            let new = m
                .dir
                .with_file_name(format!("{:0width$}-{}", m.id.0, m.name));

            renames.push(Rename { from: old, to: new });
        }

        renames
    }
}

fn available_migrations(dir: &Path) -> Result<Vec<MigrationDirectory>, IndexError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,

        // Avoid a useless error if the directory doesn't exist.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }

        Err(err) => {
            return Err(IndexError::ReadDir {
                path: dir.to_path_buf(),
                err,
            });
        }
    };

    lazy_static! {
        static ref RE_MIGRATION: Regex =
            Regex::new(r"^(?P<id>\d+)-(?P<name>.*)$").expect("static pattern");
    }

    let paths: Vec<MigrationDirectory> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() {
                // TODO: log skip on errors
                let m = RE_MIGRATION.captures(path.file_name()?.to_str()?)?;

                let id = m.name("id")?.as_str().parse().ok()?;
                let name = m.name("name")?.as_str().to_string();

                Some(MigrationDirectory {
                    id,
                    name,
                    dir: path.clone(),
                    up_path: path.join("up.sql"),
                    down_path: path.join("down.sql"),
                })
            } else {
                // TODO: log skip because not dir
                None
            }
        })
        .collect();

    Ok(paths)
}

#[derive(thiserror::Error, Debug)]
pub enum IoError {
    #[error("failed to create directory: {0}: {1}")]
    CreateDir(PathBuf, std::io::Error),

    #[error("failed to create file: {0}: {1}")]
    CreateFile(PathBuf, std::io::Error),

    #[error("failed to write file: {0}: {1}")]
    WriteFile(PathBuf, std::io::Error),
}

fn create_migration_files(
    dir: &Path,
    up_sql: String,
    down_sql: String,
) -> Result<MigrationFiles, IoError> {
    let up_path = dir.join("up.sql");
    let down_path = dir.join("down.sql");

    tracing::info!("Creating migration directory: {}", dir.to_string_lossy());
    mkdir(dir)?;

    tracing::info!("Creating up migration file: {}", up_path.to_string_lossy());
    create_file(&up_path, &up_sql)?;

    tracing::info!(
        "Creating down migration file: {}",
        down_path.to_string_lossy()
    );
    create_file(&down_path, &down_sql)?;

    Ok(MigrationFiles {
        up: up_path,
        down: down_path,
    })
}

fn mkdir(path: &Path) -> Result<(), IoError> {
    std::fs::create_dir_all(path).map_err(|err| IoError::CreateDir(path.to_path_buf(), err))
}

fn create_file(path: &Path, content: &str) -> Result<(), IoError> {
    std::fs::File::create(path)
        .map_err(|err| IoError::CreateFile(path.to_path_buf(), err))?
        .write_all(content.as_bytes())
        .map_err(|err| IoError::WriteFile(path.to_path_buf(), err))
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use super::*;

    #[tokio::test]
    async fn empty() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        let index = MigrationIndex::new(&config.migrations_dir).unwrap();
        assert!(index.index.is_empty(), "{index:?}");
    }

    #[tokio::test]
    async fn no_root_directory() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        std::fs::remove_dir(&config.migrations_dir).unwrap();

        let index = MigrationIndex::new(&config.migrations_dir).unwrap();

        assert_eq!(config.migrations_dir, index.dir);
        assert_eq!(0, index.index.len());
    }

    #[tokio::test]
    async fn duplicate_migration_ids() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        mkdir(&config.migrations_dir.join("123-first")).unwrap();
        mkdir(&config.migrations_dir.join("123-second")).unwrap();

        match MigrationIndex::new(&config.migrations_dir) {
            Err(IndexError::MultipleMigrationDirectories(map)) => {
                let mut expected = vec![
                    MigrationDirectory {
                        id: MigrationId(123),
                        name: String::from("first"),
                        dir: config.migrations_dir.join("123-first"),
                        up_path: config.migrations_dir.join("123-first/up.sql"),
                        down_path: config.migrations_dir.join("123-first/down.sql"),
                    },
                    MigrationDirectory {
                        id: MigrationId(123),
                        name: String::from("second"),
                        dir: config.migrations_dir.join("123-second"),
                        up_path: config.migrations_dir.join("123-second/up.sql"),
                        down_path: config.migrations_dir.join("123-second/down.sql"),
                    },
                ];
                expected.sort();

                let mut actual = map.get(&MigrationId(123)).unwrap().clone();
                actual.sort();

                assert_eq!(expected, actual);
            }
            Ok(index) => panic!("Index built from invalid state: {index:?}"),
            Err(err) => panic!("{err:?}"),
        }
    }

    #[tokio::test]
    async fn extra_files() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        mkdir(&config.migrations_dir.join("not-a-migration-dir")).unwrap();
        create_file(&config.migrations_dir.join(".gitignore"), "some_file").unwrap();

        let index = MigrationIndex::new(&config.migrations_dir).unwrap();
        assert!(index.index.is_empty(), "{index:?}");
    }

    #[tokio::test]
    async fn migration_id_already_exists() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        mkdir(&config.migrations_dir.join("123-first")).unwrap();

        let mut index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let params = MigrationParams {
            id: MigrationId(123),
            name: String::from("second"),
            up_sql: String::from("-- 123-second: up"),
            down_sql: String::from("-- 123-second: down"),
        };

        match index.create(params) {
            Err(CreateMigrationError::ExistingDirectory(migration)) => {
                assert_eq!(MigrationId(123), migration.id);
                assert_eq!("first", &migration.name);
            }
            Ok(files) => panic!("Colliding migration files created: {files:?}"),
            Err(err) => panic!("{err:?}"),
        };
    }

    #[tokio::test]
    async fn create_migration() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        let mut index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let params = MigrationParams {
            id: MigrationId(123),
            name: String::from("first"),
            up_sql: String::from("-- 123-first: up"),
            down_sql: String::from("-- 123-first: down"),
        };

        let files = index.create(params.clone()).unwrap();

        let actual_up_sql = std::fs::read_to_string(files.up_path).unwrap();
        let actual_down_sql = std::fs::read_to_string(files.down_path).unwrap();

        assert_eq!(&params.up_sql, &actual_up_sql);
        assert_eq!(&params.down_sql, &actual_down_sql);

        let migration = index.get(MigrationId(123)).unwrap();

        assert_eq!("first", &migration.name);
        assert_eq!(config.migrations_dir.join("123-first"), migration.dir);
    }

    #[tokio::test]
    async fn align_id_add_padding() {
        let env = TestEnv::new().await.unwrap();
        let config = env.config();

        mkdir(&config.migrations_dir.join("0-init")).unwrap();
        mkdir(&config.migrations_dir.join("1-create_users")).unwrap();
        mkdir(&config.migrations_dir.join("02-manually_padded")).unwrap();
        mkdir(&config.migrations_dir.join("1234567890-unix_timestamp")).unwrap();

        let index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let mut actual = index.align_ids();
        actual.sort();

        let realpath = |base: &str| -> PathBuf {
            let base: PathBuf = base.parse().unwrap();
            config.migrations_dir.join(base)
        };

        let mut expected = vec![
            Rename {
                from: realpath("0-init"),
                to: realpath("0000000000-init"),
            },
            Rename {
                from: realpath("1-create_users"),
                to: realpath("0000000001-create_users"),
            },
            Rename {
                from: realpath("02-manually_padded"),
                to: realpath("0000000002-manually_padded"),
            },
            Rename {
                from: realpath("1234567890-unix_timestamp"),
                to: realpath("1234567890-unix_timestamp"),
            },
        ];
        expected.sort();

        assert_eq!(expected, actual);
    }
}
