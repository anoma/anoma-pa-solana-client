//! pa-evm's `uint256` as the adapter's events carry it.

/// A 256-bit unsigned integer: its 32 little-endian bytes, the IDL's `u256`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct U256(pub [u8; 32]);

/// A [`U256`] larger than the integer it was converted to holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct U256OutOfRange(pub U256);

impl core::fmt::Display for U256OutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "the u256 {:02x?} (little-endian) is out of range",
            self.0 .0
        )
    }
}

impl std::error::Error for U256OutOfRange {}

impl TryFrom<U256> for u32 {
    type Error = U256OutOfRange;

    fn try_from(value: U256) -> Result<u32, U256OutOfRange> {
        let (low, high) = value.0.split_at(4);
        if high.iter().any(|byte| *byte != 0) {
            return Err(U256OutOfRange(value));
        }
        Ok(u32::from_le_bytes(
            low.try_into().expect("the split's low part is 4 bytes"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_u256_within_u32_converts_from_its_low_bytes() {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&[0x04, 0x03, 0x02, 0x01]);
        assert_eq!(u32::try_from(U256(bytes)), Ok(0x0102_0304));
        assert_eq!(u32::try_from(U256([0; 32])), Ok(0));
    }

    #[test]
    fn a_u256_past_u32_does_not_convert() {
        for byte in [4, 31] {
            let mut bytes = [0u8; 32];
            bytes[byte] = 1;
            assert_eq!(
                u32::try_from(U256(bytes)),
                Err(U256OutOfRange(U256(bytes))),
                "byte {byte} set"
            );
        }
    }
}
