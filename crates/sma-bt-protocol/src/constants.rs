//! Protocol constants. Sourced from observing the SBFspot reference.

/// Start/end delimiter for SMA BT frames (HDLC-style).
pub const FRAME_DELIMITER: u8 = 0x7E;

/// Layer-2 signature on Bluetooth transport (little-endian wire = FF 03 60 65).
pub const BT_L2_SIGNATURE: u32 = 0x65_60_03_FF;

/// Layer-2 signature on Ethernet/Speedwire transport. Kept here for symmetry;
/// this crate only implements BT today.
pub const ETH_L2_SIGNATURE: u32 = 0x65_60_10_00;

/// "Any SUSyID" wildcard used when destination is a broadcast scan.
pub const ANY_SUSY_ID: u16 = 0xFFFF;

/// "Any serial" wildcard.
pub const ANY_SERIAL: u32 = 0xFFFFFFFF;

/// SUSyID we use to identify ourselves to the inverter. 125 matches the
/// historic SBFspot value — SMA treats any unregistered SUSyID as an app.
pub const APP_SUSY_ID: u16 = 125;

/// Packet ID bit we set to mark our packets as application-originated.
pub const APP_PACKET_BIT: u16 = 0x8000;

/// BT RFCOMM channel SMA inverters listen on.
pub const RFCOMM_CHANNEL: u8 = 1;

/// Broadcast BT address (all zeros).
pub const BROADCAST_BT: [u8; 6] = [0; 6];

/// "Unknown peer" BT address (all 0xFF) — used early in discovery.
pub const UNKNOWN_BT: [u8; 6] = [0xFF; 6];
