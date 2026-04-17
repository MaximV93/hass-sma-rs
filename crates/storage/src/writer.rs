//! Async writer against a TimescaleDB instance.
//!
//! Kept deliberately thin: a single INSERT per reading. At daemon startup
//! we call `init_schema` to ensure the hypertable exists. The pool is sized
//! for ~20 concurrent inserts which is well above what a few inverters
//! produce even at 5 s polling.

use chrono::{DateTime, Utc};
use sqlx::{postgres::PgPoolOptions, PgPool};
use thiserror::Error;

use crate::schema::CREATE_SCHEMA;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, StorageError>;

pub struct StorageWriter {
    pool: PgPool,
}

impl StorageWriter {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Create/upgrade schema. Idempotent.
    pub async fn init_schema(&self) -> Result<()> {
        // sqlx::query doesn't accept multiple statements; split on `;`.
        for stmt in CREATE_SCHEMA
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sqlx::query(stmt).execute(&self.pool).await?;
        }
        Ok(())
    }

    /// Insert one reading.
    pub async fn insert(
        &self,
        time: DateTime<Utc>,
        slot: &str,
        serial: i64,
        metric: &str,
        value: f64,
    ) -> Result<()> {
        sqlx::query("INSERT INTO inverter_readings (time, slot, serial, metric, value) VALUES ($1, $2, $3, $4, $5)")
            .bind(time)
            .bind(slot)
            .bind(serial)
            .bind(metric)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_nonempty() {
        assert!(CREATE_SCHEMA.contains("CREATE TABLE"));
        assert!(CREATE_SCHEMA.contains("hypertable"));
        assert!(CREATE_SCHEMA.contains("add_retention_policy"));
        assert!(CREATE_SCHEMA.contains("hourly_avg"));
    }
}
