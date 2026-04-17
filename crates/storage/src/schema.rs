//! Schema DDL for TimescaleDB. Call via `StorageWriter::init_schema`.

/// Idempotent DDL that creates the `inverter_readings` hypertable, its
/// retention + compression policies, and the `hourly_avg` continuous
/// aggregate. Safe to call on every daemon start.
pub const CREATE_SCHEMA: &str = r#"
CREATE EXTENSION IF NOT EXISTS timescaledb;

CREATE TABLE IF NOT EXISTS inverter_readings (
    time    TIMESTAMPTZ      NOT NULL,
    slot    TEXT             NOT NULL,
    serial  BIGINT           NOT NULL,
    metric  TEXT             NOT NULL,
    value   DOUBLE PRECISION NOT NULL
);

SELECT create_hypertable('inverter_readings', 'time', if_not_exists => TRUE);

CREATE INDEX IF NOT EXISTS idx_inv_slot_metric
    ON inverter_readings (slot, metric, time DESC);

SELECT add_retention_policy('inverter_readings', INTERVAL '90 days', if_not_exists => TRUE);

ALTER TABLE inverter_readings
    SET (timescaledb.compress,
         timescaledb.compress_segmentby = 'slot,metric');

SELECT add_compression_policy('inverter_readings', INTERVAL '7 days', if_not_exists => TRUE);

CREATE MATERIALIZED VIEW IF NOT EXISTS hourly_avg
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', time) AS bucket,
    slot,
    metric,
    AVG(value) AS avg_value,
    MAX(value) AS max_value,
    MIN(value) AS min_value,
    COUNT(*)   AS samples
FROM inverter_readings
GROUP BY bucket, slot, metric
WITH NO DATA;

SELECT add_continuous_aggregate_policy('hourly_avg',
    start_offset      => INTERVAL '1 day',
    end_offset        => INTERVAL '1 hour',
    schedule_interval => INTERVAL '30 minutes',
    if_not_exists     => TRUE);
"#;
