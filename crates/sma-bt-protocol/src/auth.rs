//! Password encoding + logon packet construction.
//!
//! SMA inverters accept a 12-byte encoded password. Each ASCII character of
//! the user-entered password is **added** (mod 256) to a per-user-group key.
//! Remaining bytes are filled with the key value.
//!
//! User groups:
//! - `UserGroup::User`      → key = 0x88
//! - `UserGroup::Installer` → key = 0xBB
//!
//! Empty password: all 12 bytes are the key value.

use crate::packet::{encode_l2, L2Header};
use byteorder::{ByteOrder, LittleEndian};

/// Which user group the logon targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserGroup {
    User,
    Installer,
}

impl UserGroup {
    /// The addition key used for password obfuscation.
    pub const fn key(self) -> u8 {
        match self {
            UserGroup::User => 0x88,
            UserGroup::Installer => 0xBB,
        }
    }

    /// Encoded as a u32 in the logon body (byte 4 of command body).
    pub const fn code(self) -> u32 {
        match self {
            UserGroup::User => 0x0000_0007, // UG_USER in SBFspot
            UserGroup::Installer => 0x0000_000A, // UG_INSTALLER
        }
    }
}

/// Encode a password for transport.
///
/// ASCII bytes are added (mod 256) to the user-group key. Output is always
/// exactly 12 bytes. Trailing bytes are the key value.
pub fn encode_password(pwd: &str, group: UserGroup) -> [u8; 12] {
    let key = group.key();
    let mut out = [key; 12];
    for (i, b) in pwd.as_bytes().iter().take(12).enumerate() {
        out[i] = b.wrapping_add(key);
    }
    out
}

/// Build the full L2 body (header + logon command) for an initial logon.
///
/// Parameters mirror the SBFspot wire format:
/// - `pkt_id`: 16-bit packet id assigned by us
/// - `app_serial`: random session id we generated
/// - `now_epoch`: current unix time (echoed back in response to pair the reply)
pub fn build_logon_body(
    pwd: &str,
    group: UserGroup,
    pkt_id: u16,
    app_serial: u32,
    now_epoch: u32,
) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(4 + 4 + 4 + 4 + 4 + 12);
    let mut tmp = [0u8; 4];

    LittleEndian::write_u32(&mut tmp, 0xFFFD_040C);
    cmd.extend_from_slice(&tmp);
    LittleEndian::write_u32(&mut tmp, group.code());
    cmd.extend_from_slice(&tmp);
    LittleEndian::write_u32(&mut tmp, 0x0000_0384); // timeout, 900 s
    cmd.extend_from_slice(&tmp);
    LittleEndian::write_u32(&mut tmp, now_epoch);
    cmd.extend_from_slice(&tmp);
    cmd.extend_from_slice(&[0u8; 4]); // reserved
    cmd.extend_from_slice(&encode_password(pwd, group));

    let hdr = L2Header::logon(pkt_id, app_serial);
    encode_l2(&hdr, &cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_password_user_group() {
        let enc = encode_password("", UserGroup::User);
        assert_eq!(enc, [0x88; 12]);
    }

    #[test]
    fn empty_password_installer_group() {
        let enc = encode_password("", UserGroup::Installer);
        assert_eq!(enc, [0xBB; 12]);
    }

    #[test]
    fn default_0000_password() {
        // ASCII "0" = 0x30; 0x30 + 0x88 = 0xB8
        let enc = encode_password("0000", UserGroup::User);
        assert_eq!(enc[..4], [0xB8, 0xB8, 0xB8, 0xB8]);
        // remaining bytes = key
        assert_eq!(enc[4..], [0x88; 8]);
    }

    #[test]
    fn password_truncated_to_12() {
        // 15-char password: only first 12 are encoded, rest ignored.
        let enc = encode_password("abcdefghijklmno", UserGroup::User);
        assert_eq!(enc[0], b'a'.wrapping_add(0x88));
        assert_eq!(enc[11], b'l'.wrapping_add(0x88));
    }

    #[test]
    fn logon_body_structure() {
        let body = build_logon_body("0000", UserGroup::User, 0x0001, 900_100_200, 1_700_000_000);

        // Starts with L2 signature
        assert_eq!(&body[..4], &[0xFF, 0x03, 0x60, 0x65]);
        // L2 header spans 28 bytes total (signature + 24 bytes header)
        assert_eq!(body[4], 0x0E); // longwords
        assert_eq!(body[5], 0xA0); // ctrl

        // Command starts at offset 28
        let cmd = &body[28..];
        assert_eq!(&cmd[0..4], &[0x0C, 0x04, 0xFD, 0xFF]); // 0xFFFD040C little-endian
        assert_eq!(&cmd[4..8], &UserGroup::User.code().to_le_bytes());
        assert_eq!(&cmd[8..12], &0x0000_0384u32.to_le_bytes());
        assert_eq!(&cmd[12..16], &1_700_000_000u32.to_le_bytes());
        assert_eq!(&cmd[16..20], &[0u8; 4]);
        // encoded "0000" password at the end
        assert_eq!(cmd[20..24], [0xB8, 0xB8, 0xB8, 0xB8]);
        assert_eq!(cmd[24..32], [0x88; 8]);
    }
}
