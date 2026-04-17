//! Per-inverter session state machine.
//!
//! Drives the SMA BT inverter through handshake, logon, and polling. Transport
//! agnostic: any `bluez_transport::Transport` drives the same code path.

pub mod session;
pub mod values;

pub use session::{Session, SessionConfig, SessionError, SessionState};
pub use values::{parse_spot_ac_total_power, InverterReadings};
