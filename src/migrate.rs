use lazy_static::lazy_static;
use regex::Regex;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, PgExecutor};
use std::path::PathBuf;

// Migration ID has to fit in an i64 for Postgres purposes, but it should always be non-negative.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct MigrationId(pub(crate) i64);

impl std::fmt::Display for MigrationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl MigrationId {
    pub fn as_i64(&self) -> i64 {
        self.0
    }
}

impl MigrationId {
    pub(crate) fn width(&self) -> usize {
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MigrationDirectory {
    pub id: MigrationId,
    pub name: String,

    pub dir: PathBuf,
    pub up_path: PathBuf,
    pub down_path: PathBuf,
}

impl std::fmt::Display for MigrationDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dir.to_string_lossy())
    }
}

pub fn skip_transaction(sql: &str) -> bool {
    lazy_static! {
        static ref RE_NO_TX: Regex =
            Regex::new("(?m)^--squill:no-transaction").expect("static pattern");
    }

    RE_NO_TX.is_match(sql)
}

pub async fn claim(
    conn: impl PgExecutor<'_>,
    id: MigrationId,
    name: &str,
) -> sqlx::Result<<sqlx::Postgres as sqlx::Database>::QueryResult> {
    let query = sqlx::query("select _squill_claim_migration($1, $2)")
        .bind(id.as_i64())
        .bind(name);

    conn.execute(query).await
}

pub async fn unclaim(
    conn: impl PgExecutor<'_>,
    id: MigrationId,
) -> sqlx::Result<<sqlx::Postgres as sqlx::Database>::QueryResult> {
    let query = sqlx::query("select _squill_unclaim_migration($1)").bind(id.as_i64());

    conn.execute(query).await
}

impl MigrationDirectory {
    pub async fn up(&self, conn: &mut PgConnection) -> Result<(), MigrateError> {
        let sql = std::fs::read_to_string(&self.up_path).map_err(MigrateError::Read)?;

        if skip_transaction(&sql) {
            conn.execute(&*sql).await.map_err(MigrateError::Execute)?;
        } else {
            let id = self.id;
            let name = self.name.clone();

            conn.transaction(|conn| {
                Box::pin(async move {
                    claim(&mut **conn, id, &name).await?;
                    conn.execute(&*sql).await
                })
            })
            .await
            .map_err(MigrateError::Execute)?;
        }

        Ok(())
    }

    // TODO: Add some sort of "forward-only" flag that prevents down migrations.
    pub async fn down(&self, conn: &mut PgConnection) -> Result<(), MigrateError> {
        let sql = std::fs::read_to_string(&self.down_path).map_err(MigrateError::Read)?;

        if skip_transaction(&sql) {
            conn.execute(&*sql).await.map_err(MigrateError::Execute)?;
        } else {
            let id = self.id;

            conn.transaction(|conn| {
                Box::pin(async move {
                    unclaim(&mut **conn, id).await?;
                    conn.execute(&*sql).await
                })
            })
            .await
            .map_err(MigrateError::Execute)?;
        }

        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum MigrateError {
    #[error("failed to read migration file: {0}")]
    Read(std::io::Error),

    #[error("failed to execute migration: {0}")]
    Execute(sqlx::Error),
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use super::*;

    #[tokio::test]
    async fn no_tx() {
        assert!(skip_transaction(NO_OP_NO_TX));
    }

    #[tokio::test]
    async fn yes_tx() {
        assert!(!skip_transaction(NO_OP_YES_TX));
    }

    #[test]
    fn migration_ids() {
        MigrationId::try_from(0).unwrap();
        MigrationId::try_from(1).unwrap();
        MigrationId::try_from(1234567890).unwrap();
        MigrationId::try_from(i64::MAX).unwrap();

        match MigrationId::try_from(-1) {
            Err(ParseMigrationIdError::Negative(_)) => (),

            Ok(id) => panic!("Unexpected success: {id}"),
            Err(err) => panic!("Unexpected error: {:?}", err),
        }

        match "-1".parse::<MigrationId>() {
            Err(ParseMigrationIdError::Negative(_)) => (),

            Ok(id) => panic!("Unexpected success: {id}"),
            Err(err) => panic!("Unexpected error: {:?}", err),
        }

        match "0x10".parse::<MigrationId>() {
            Err(ParseMigrationIdError::ParseInt(_)) => (),

            Ok(id) => panic!("Unexpected success: {id}"),
            Err(err) => panic!("Unexpected error: {:?}", err),
        }

        match "a0".parse::<MigrationId>() {
            Err(ParseMigrationIdError::ParseInt(_)) => (),

            Ok(id) => panic!("Unexpected success: {id}"),
            Err(err) => panic!("Unexpected error: {:?}", err),
        }
    }
}
