//! FCS-16 checksum used by SMA BT L2 payloads.
//!
//! This is the standard PPP FCS-16 (ISO/IEC 3309, RFC 1662 §C.2).
//! Polynomial: `x^16 + x^12 + x^5 + 1` (0x1021), LSB-first, initial 0xFFFF,
//! final XOR with 0xFFFF.
//!
//! The table below is derived from that polynomial; it is identical to the
//! table in every PPP implementation (e.g. Linux `net/ppp/ppp_async.c`,
//! FreeBSD `sys/net/ppp_tty.c`) and therefore not SBFspot-specific.

const FCS_TAB: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut b = 0u16;
    while b < 256 {
        let mut v = b;
        let mut i = 0;
        while i < 8 {
            if v & 1 != 0 {
                v = (v >> 1) ^ 0x8408;
            } else {
                v >>= 1;
            }
            i += 1;
        }
        table[b as usize] = v;
        b += 1;
    }
    table
};

/// Streaming FCS-16 calculator.
#[derive(Debug, Clone, Copy)]
pub struct Fcs16(u16);

impl Default for Fcs16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fcs16 {
    /// New calculator, pre-initialised with 0xFFFF.
    #[inline]
    pub const fn new() -> Self {
        Self(0xFFFF)
    }

    /// Absorb a single byte.
    #[inline]
    pub fn update(&mut self, byte: u8) {
        self.0 = (self.0 >> 8) ^ FCS_TAB[((self.0 ^ byte as u16) & 0xFF) as usize];
    }

    /// Absorb a slice.
    #[inline]
    pub fn update_slice(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.update(b);
        }
    }

    /// Finalise (XOR with 0xFFFF) and return the checksum as emitted on the wire.
    #[inline]
    pub const fn finalize(self) -> u16 {
        self.0 ^ 0xFFFF
    }
}

/// Convenience: compute FCS-16 over a byte slice in one call.
#[inline]
pub fn compute(data: &[u8]) -> u16 {
    let mut f = Fcs16::new();
    f.update_slice(data);
    f.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ppp_vector() {
        // Standard PPP FCS test vector: "123456789" -> 0x906E
        //   (many RFC implementations use 0x6F91 before XOR-final; final = 0x906E)
        assert_eq!(compute(b"123456789"), 0x906E);
    }

    #[test]
    fn empty_is_zero() {
        // Empty input → 0xFFFF before finalize → 0 after XOR
        assert_eq!(compute(b""), 0);
    }

    #[test]
    fn known_sbf_table_row_zero() {
        // First row of the fcstab in the reference must match what our generator produced.
        assert_eq!(FCS_TAB[0], 0x0000);
        assert_eq!(FCS_TAB[1], 0x1189);
        assert_eq!(FCS_TAB[2], 0x2312);
        assert_eq!(FCS_TAB[255], 0x0F78);
    }
}
