//! TimescaleDB writer + MariaDB-SpotData migration.
//!
//! Schema (PostgreSQL + TimescaleDB hypertable):
//!
//! ```sql
//! CREATE TABLE inverter_readings (
//!     time        TIMESTAMPTZ     NOT NULL,
//!     slot        TEXT            NOT NULL,
//!     serial      BIGINT          NOT NULL,
//!     metric      TEXT            NOT NULL,
//!     value       DOUBLE PRECISION NOT NULL,
//!     PRIMARY KEY (time, slot, metric)
//! );
//! SELECT create_hypertable('inverter_readings', 'time');
//! CREATE INDEX idx_inv_slot_metric ON inverter_readings (slot, metric, time DESC);
//!
//! -- Retention: drop raw rows > 90 days
//! SELECT add_retention_policy('inverter_readings', INTERVAL '90 days');
//!
//! -- Compress rows after 7 days (~90% storage reduction)
//! ALTER TABLE inverter_readings SET (timescaledb.compress, timescaledb.compress_segmentby = 'slot,metric');
//! SELECT add_compression_policy('inverter_readings', INTERVAL '7 days');
//!
//! -- Continuous aggregate: hourly averages kept forever
//! CREATE MATERIALIZED VIEW hourly_avg
//! WITH (timescaledb.continuous) AS
//! SELECT time_bucket('1 hour', time) AS bucket, slot, metric, AVG(value) AS avg_value, MAX(value) AS max_value
//! FROM inverter_readings
//! GROUP BY bucket, slot, metric;
//!
//! SELECT add_continuous_aggregate_policy('hourly_avg',
//!     start_offset => INTERVAL '1 day',
//!     end_offset   => INTERVAL '1 hour',
//!     schedule_interval => INTERVAL '30 minutes');
//! ```

pub mod schema;
pub mod writer;

pub use schema::CREATE_SCHEMA;
pub use writer::{StorageError, StorageWriter};
