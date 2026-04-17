//! Per-inverter identity used as input to discovery building.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// Actual solar inverter (Sunny Boy family).
    SolarInverter,
    /// BT repeater device that SMA exposes on the piconet.
    BluetoothRepeater,
    /// Battery-tied inverter (Sunny Island, STP xxSE etc.).
    BatteryInverter,
}

/// Stable identity for one inverter. Used as `device.identifiers` in HA
/// discovery so HA groups all its sensors under one device card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InverterIdentity {
    /// Short operator-assigned slot name, e.g. "zolder" / "garage". Used
    /// in the entity object_id: `sensor.sbfspot_<slot>_<metric>`.
    pub slot: String,
    /// The inverter's serial number (decimal). Used as unique_id disambiguator.
    pub serial: u32,
    /// Human-readable model (e.g. "SB 3000HF-30").
    pub model: String,
    /// Firmware version string (e.g. "02.30.06.R").
    pub firmware: String,
    /// SMA device class.
    pub kind: DeviceKind,
}
