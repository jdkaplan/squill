use std::collections::BTreeMap;

use crate::config::{Config, ConnectError};
use crate::db::{MigrationLog, MigrationRecord, QueryError};
use crate::index::{IndexError, IoError, MigrationIndex};
use crate::migrate::{MigrationDirectory, MigrationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub applied: MigrationLog,
    pub available: MigrationIndex,
}

impl Status {
    pub async fn new(config: &Config) -> Result<Self, StatusError> {
        let mut conn = config.connect().await.map_err(StatusError::Connect)?;

        let applied = MigrationLog::new(&mut conn)
            .await
            .map_err(StatusError::Query)?;

        let available = MigrationIndex::new(&config.migrations_dir).map_err(StatusError::Index)?;

        Ok(Self { applied, available })
    }

    pub fn pending(&self) -> Vec<MigrationDirectory> {
        self.available
            .iter()
            .cloned()
            .filter(|m| !self.applied.log.contains_key(&m.id))
            .collect()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StatusError {
    #[error(transparent)]
    Connect(ConnectError),

    #[error(transparent)]
    Query(QueryError),

    #[error(transparent)]
    Io(IoError),

    #[error(transparent)]
    Index(IndexError),
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub id: MigrationId,
    pub name: String,
    pub run_at: Option<time::PrimitiveDateTime>,
    pub directory: Option<String>,
}

impl Status {
    pub fn full_status(&self) -> BTreeMap<MigrationId, StatusEntry> {
        let mut entries = BTreeMap::new();

        for (id, (row, dir)) in self.collate() {
            entries.insert(id, Self::status_entry(id, row, dir));
        }

        entries
    }

    fn status_entry(
        id: MigrationId,
        row: Option<MigrationRecord>,
        dir: Option<MigrationDirectory>,
    ) -> StatusEntry {
        match (row, dir) {
            (Some(row), Some(dir)) => StatusEntry {
                id,
                name: row.name.clone(),
                run_at: Some(row.run_at),
                directory: Some(dir.to_string()),
            },
            (Some(row), None) => StatusEntry {
                id,
                name: row.name.clone(),
                run_at: Some(row.run_at),
                directory: None,
            },
            (None, Some(dir)) => StatusEntry {
                id,
                name: dir.name.clone(),
                run_at: None,
                directory: Some(dir.to_string()),
            },
            (None, None) => unreachable!("empty status entry for id: {id}"),
        }
    }

    fn collate(
        &self,
    ) -> BTreeMap<MigrationId, (Option<MigrationRecord>, Option<MigrationDirectory>)> {
        let mut zipped = BTreeMap::new();

        for migration in self.applied.iter() {
            let (applied, _) = zipped.entry(migration.id).or_insert((None, None));
            *applied = Some(migration.clone());
        }

        for migration in self.available.iter() {
            let (_, available) = zipped.entry(migration.id).or_insert((None, None));
            *available = Some(migration.clone());
        }

        zipped
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use super::*;

    #[tokio::test]
    async fn status_empty() {
        let env = TestEnv::new().await.unwrap();

        let actual = Status::new(&env.config()).await.unwrap();

        assert_eq!(None, actual.applied.iter().next());
        assert_eq!(None, actual.available.iter().next());
    }

    #[tokio::test]
    async fn pending_migrations() {
        let env = TestEnv::initialized().await.unwrap();
        let config = env.config();

        let mut index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let _ = index.create(fake_migration(1, "one")).unwrap();
        let two = index.create(fake_migration(2, "two")).unwrap();
        let _ = index.create(fake_migration(3, "three")).unwrap();

        let mut conn = config.connect().await.unwrap();
        two.up(&mut conn).await.unwrap();

        let status = Status::new(&config).await.unwrap();
        let actual = status.pending();

        let expected = vec![
            // 0-init applied
            index.get(MigrationId(1)).unwrap().clone(),
            // 2-two applied
            index.get(MigrationId(3)).unwrap().clone(),
        ];

        assert_eq!(expected, actual);
    }

    #[tokio::test]
    async fn status_entries() {
        let env = TestEnv::initialized().await.unwrap();
        let config = env.config();

        let mut index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let one = index.create(fake_migration(1, "one")).unwrap();
        let _ = index.create(fake_migration(2, "two")).unwrap();

        let mut conn = config.connect().await.unwrap();
        one.up(&mut conn).await.unwrap();

        std::fs::remove_dir_all(&one.dir).unwrap();

        let status = Status::new(&config).await.unwrap();
        let actual = status.full_status();

        assert_eq!(3, actual.len());

        // Applied and still present
        {
            let zero = actual.get(&MigrationId(0)).unwrap();
            assert_eq!(MigrationId(0), zero.id);
            assert_eq!("init", &zero.name);
            assert!(zero.run_at.is_some());
            assert!(zero.directory.is_some());
        }

        // Deleted after applying
        {
            let one = actual.get(&MigrationId(1)).unwrap();
            assert_eq!(MigrationId(1), one.id);
            assert_eq!("one", &one.name);
            assert!(one.run_at.is_some());
            assert_eq!(None, one.directory);
        }

        // Not applied
        {
            let two = actual.get(&MigrationId(2)).unwrap();
            assert_eq!(MigrationId(2), two.id);
            assert_eq!("two", &two.name);
            assert_eq!(None, two.run_at);
            assert!(two.directory.is_some());
        }
    }
}
