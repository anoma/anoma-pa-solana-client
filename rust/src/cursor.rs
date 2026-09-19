//! Cursor over Borsh-encoded bytes, shared by the account and event decoders.
//! Walks the schema field by field; no hardcoded offsets.

use crate::constants::HASH_LEN;

/// The data ran out while reading the named field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Truncated {
    pub field: &'static str,
}

pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], Truncated> {
        let end = self.pos.checked_add(len).ok_or(Truncated { field })?;
        let slice = self.data.get(self.pos..end).ok_or(Truncated { field })?;
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self, field: &'static str) -> Result<u8, Truncated> {
        Ok(self.take(1, field)?[0])
    }

    pub fn u32_le(&mut self, field: &'static str) -> Result<u32, Truncated> {
        let bytes: [u8; 4] = self.take(4, field)?.try_into().expect("4 bytes");
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn u64_le(&mut self, field: &'static str) -> Result<u64, Truncated> {
        let bytes: [u8; 8] = self.take(8, field)?.try_into().expect("8 bytes");
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn array_32(&mut self, field: &'static str) -> Result<[u8; 32], Truncated> {
        Ok(self.take(HASH_LEN, field)?.try_into().expect("32 bytes"))
    }

    /// Borsh `Vec<u8>`: u32 LE length, then the bytes.
    pub fn vec_u8(&mut self, field: &'static str) -> Result<Vec<u8>, Truncated> {
        let len = self.u32_le(field)? as usize;
        Ok(self.take(len, field)?.to_vec())
    }

    /// Borsh `Vec<[u8; 32]>`: u32 LE length, then the arrays.
    pub fn vec_array_32(&mut self, field: &'static str) -> Result<Vec<[u8; 32]>, Truncated> {
        let len = self.u32_le(field)? as usize;
        (0..len).map(|_| self.array_32(field)).collect()
    }
}
