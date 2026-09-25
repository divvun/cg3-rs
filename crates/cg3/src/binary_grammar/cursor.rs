//! A bounds-checked read position over a whole `.cg3b`.
//!
//! The C++ reads through an `istream` with its exception mask set, so a short
//! read throws and the process dies. The port reads from the loaded bytes and
//! reports a short read as [`GrammarError::Truncated`], naming the field and
//! where it started; a number checked here is refused with
//! [`GrammarError::BinaryMalformed`] at the offset it was read from.

use crate::error::{BinaryFault, GrammarError};
use crate::inlines::{ByteOrdered, read_be_f64};

/// A `.cg3b` fault at `offset`.
pub(crate) fn malformed(offset: usize, fault: BinaryFault) -> GrammarError {
    GrammarError::BinaryMalformed { offset, fault }
}

/// The two keys the flat hash containers reserve as empty/deleted markers.
pub(crate) fn is_reserved_hash(hash: u32) -> bool {
    hash >= u32::MAX - 1
}

/// Reads big-endian fields out of a `.cg3b`, refusing any read past its end.
pub(crate) struct Cg3bCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cg3bCursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Cg3bCursor { data, pos: 0 }
    }

    /// The byte offset the next read starts at.
    pub(crate) fn offset(&self) -> usize {
        self.pos
    }

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// The next `n` bytes, or [`GrammarError::Truncated`] if fewer remain.
    pub(crate) fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], GrammarError> {
        let remaining = self.data.len() - self.pos;
        if n > remaining {
            return Err(GrammarError::Truncated {
                what,
                offset: self.pos,
                needed: n as u64,
                remaining,
            });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    /// One big-endian integer (C++ `readBE<T>`).
    pub(crate) fn be<T: ByteOrdered>(&mut self, what: &'static str) -> Result<T, GrammarError> {
        let b = self.bytes(T::byte_size(), what)?;
        Ok(T::from_be_slice(b))
    }

    /// The 12-byte double (`u64` mantissa + `i32` exponent) of
    /// [`read_be_f64`].
    pub(crate) fn f64(&mut self, what: &'static str) -> Result<f64, GrammarError> {
        let mut b = self.bytes(12, what)?;
        Ok(read_be_f64(&mut b))
    }

    /// A `u32` length and that many bytes, decoded as UTF-8 the way the C++
    /// converter does: malformed sequences become U+FFFD.
    pub(crate) fn text(&mut self, what: &'static str) -> Result<String, GrammarError> {
        let len: u32 = self.be(what)?;
        let b = self.bytes(len as usize, what)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    // [spec:cg3:req:robustness.allocation-bounded]
    /// A `u32` record count, refused unless the bytes left could hold that
    /// many records of at least `min_size` bytes each — so nothing sized by
    /// it can outgrow the file, and no loop over it can outlast the bytes.
    pub(crate) fn count(
        &mut self,
        what: &'static str,
        min_size: usize,
    ) -> Result<u32, GrammarError> {
        let offset = self.pos;
        let count: u32 = self.be(what)?;
        let needed = u64::from(count) * min_size as u64;
        let remaining = self.data.len() - self.pos;
        if needed > remaining as u64 {
            return Err(GrammarError::CountPastEnd {
                what,
                count,
                offset,
                needed,
                remaining,
            });
        }
        Ok(count)
    }

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// A `u32` that indexes a table of `limit` entries.
    pub(crate) fn index(&mut self, what: &'static str, limit: u32) -> Result<u32, GrammarError> {
        let offset = self.pos;
        let value: u32 = self.be(what)?;
        if value >= limit {
            return Err(malformed(
                offset,
                BinaryFault::OutOfRange {
                    what,
                    value: value.into(),
                    limit: limit.into(),
                },
            ));
        }
        Ok(value)
    }

    /// A `u32` hash that may become a flat-container key, refused if it is
    /// one of their sentinels.
    pub(crate) fn hash(&mut self, what: &'static str) -> Result<u32, GrammarError> {
        let offset = self.pos;
        let hash: u32 = self.be(what)?;
        if is_reserved_hash(hash) {
            return Err(malformed(offset, BinaryFault::ReservedHash { what, hash }));
        }
        Ok(hash)
    }
}
