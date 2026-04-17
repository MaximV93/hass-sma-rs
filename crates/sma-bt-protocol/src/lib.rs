//! # sma-bt-protocol
//!
//! Clean-room implementation of the SMA proprietary Bluetooth protocol used
//! by Sunny Boy HF-series inverters (tested: SB 2000HF-30, SB 3000HF-30).
//!
//! Reverse-engineered from the SBFspot reference implementation (CC BY-NC-SA
//! 3.0, cited as documentation source only — no source code was copied).
//!
//! ## Wire format
//!
//! ```text
//! Frame = L1Header || L2Payload (byte-stuffed + FCS-terminated) || 0x7E
//!
//! L1Header (18 bytes, NOT byte-stuffed, NOT part of FCS):
//!   0x7E                        (1) frame-start
//!   len_lo len_hi               (2) total frame length, little-endian
//!   hdr_cks                     (1) = 0x7E ^ len_lo ^ len_hi
//!   local_bt[6]                 (6) sender BT MAC, little-endian order
//!   dest_bt[6]                  (6) destination BT MAC, little-endian order
//!   ctrl_lo ctrl_hi             (2) BT control word
//!
//! L2Payload (byte-stuffed, checksummed with FCS-16):
//!   0xFF 0x03 0x60 0x65         SMA L2 signature (BTH_L2SIGNATURE, LE)
//!   longwords                   (1) payload size /4
//!   ctrl1                       (1) sub-command control
//!   dstSUSyID[2] dstSerial[4]   dest "SMA SUSy" identifier
//!   ctrl2[2]
//!   AppSUSyID[2] AppSerial[4]   our identifier (SUSyID = 125, serial = random)
//!   ctrl2[2]                    repeated
//!   0x00 0x00 0x00 0x00         two reserved words
//!   pcktID[2]                   packet id with high bit set (|0x8000)
//!   <command body...>
//!
//! L2 trailer: FCS-16 checksum (2 bytes LE, after XOR'ing with 0xFFFF)
//! ```
//!
//! Byte stuffing (XOR 0x20 after 0x7D) applies to: 0x7D, 0x7E, 0x11, 0x12, 0x13.

pub mod auth;
pub mod commands;
pub mod constants;
pub mod fcs;
pub mod frame;
pub mod packet;

pub use auth::{build_logon_body, encode_password, UserGroup};
pub use commands::{build_query_body, QueryKind};
pub use constants::*;
pub use frame::{parse_l2_only_blob, Frame, FrameBuilder, FrameKind, ParseError};
pub use packet::{decode_l2, encode_l2, L2Header};

/// Byte values that require stuffing when sent inside a byte-stuffed payload.
pub const STUFF_BYTES: &[u8] = &[0x7D, 0x7E, 0x11, 0x12, 0x13];

/// Escape character inserted before a stuffed byte (which is then XOR'd with `STUFF_XOR`).
pub const STUFF_ESCAPE: u8 = 0x7D;

/// XOR mask applied to a stuffed byte after the escape character.
pub const STUFF_XOR: u8 = 0x20;
