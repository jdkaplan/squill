use std::collections::BTreeMap;

use sqlx::postgres::PgConnection;

use crate::MigrationId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRecord {
    pub id: MigrationId,
    pub name: String,
    pub run_at: time::PrimitiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationLog {
    pub(crate) log: BTreeMap<MigrationId, MigrationRecord>,
}

impl MigrationLog {
    pub async fn new(conn: &mut PgConnection) -> Result<Self, QueryError> {
        let applied = applied_migrations(conn).await?;

        let index = applied
            .into_iter()
            .map(|row| {
                (
                    MigrationId(row.id),
                    MigrationRecord {
                        id: MigrationId(row.id),
                        name: row.name,
                        run_at: row.run_at,
                    },
                )
            })
            .collect();

        Ok(Self { log: index })
    }

    pub fn iter(&self) -> impl Iterator<Item = &MigrationRecord> {
        self.log.values()
    }

    pub fn last(&self) -> Option<MigrationRecord> {
        self.iter().cloned().max_by_key(|row| (row.run_at, row.id))
    }
}

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, Eq)]
struct MigrationRow {
    pub id: i64,
    pub name: String,
    pub run_at: time::PrimitiveDateTime,
}

async fn applied_migrations(conn: &mut PgConnection) -> Result<Vec<MigrationRow>, QueryError> {
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
            Err(QueryError(err))
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("failed to query applied migrations: {0}")]
pub struct QueryError(sqlx::Error);

#[cfg(test)]
mod tests {
    use sqlx::Executor;

    use crate::testing::*;
    use crate::MigrationIndex;

    use super::*;

    #[tokio::test]
    async fn missing_table() {
        let env = TestEnv::new().await.unwrap();

        let config = env.config();
        let mut conn = config.connect().await.unwrap();

        conn.execute("drop table if exists schema_migrations")
            .await
            .unwrap();

        let log = MigrationLog::new(&mut conn).await.unwrap();
        assert!(log.log.is_empty(), "{:?}", log);
    }

    #[tokio::test]
    async fn last_applied_uninit() {
        let env = TestEnv::new().await.unwrap();

        let config = env.config();
        let mut conn = config.connect().await.unwrap();

        let last = MigrationLog::new(&mut conn).await.unwrap().last();
        assert_eq!(None, last);
    }

    #[tokio::test]
    async fn last_applied_init() {
        let env = TestEnv::initialized().await.unwrap();

        let config = env.config();
        let mut conn = config.connect().await.unwrap();

        let last = MigrationLog::new(&mut conn).await.unwrap().last();
        assert!(last.is_some());

        let last = last.unwrap();
        assert_eq!(MigrationId(0), last.id);
        assert_eq!("init", &last.name);
    }

    #[tokio::test]
    async fn last_applied_out_of_order() {
        let env = TestEnv::initialized().await.unwrap();
        let config = env.config();

        let mut index = MigrationIndex::new(&config.migrations_dir).unwrap();

        let one = index.create(fake_migration(1, "one")).unwrap();
        let two = index.create(fake_migration(2, "two")).unwrap();
        let _ = index.create(fake_migration(3, "three")).unwrap();

        // Apply "2-two" _before_ "1-one".
        let mut conn = config.connect().await.unwrap();
        two.up(&mut conn).await.unwrap();
        one.up(&mut conn).await.unwrap();

        let last = MigrationLog::new(&mut conn).await.unwrap().last();
        assert!(last.is_some());

        let last = last.unwrap();
        assert_eq!(MigrationId(1), last.id);
        assert_eq!("one", &last.name);
    }
}
