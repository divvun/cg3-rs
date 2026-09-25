//! The binary stream's checked wire primitives: a bounds-checked cursor for
//! reading a window packet, and the per-window tag table and fixed-width
//! counts for writing one.
//!
//! A stream is untrusted input however it was produced, and the format's
//! fixed-width fields cannot hold every window, so every read here is checked
//! against the body it reads from and every count against the width it is
//! stored in.

use std::collections::HashMap;
use std::io::Read;

use crate::arena::TagId;
use crate::error::RunError;

/// Which field of a binary stream window a [`BinaryStreamFault`] concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryField {
    /// The `u32` byte length that opens a window packet.
    Length,
    /// The body that length announces.
    Body,
    WindowFlags,
    TagTable,
    Variable,
    WindowText,
    CohortCount,
    CohortFlags,
    Wordform,
    StaticTag,
    Dependency,
    Relation,
    CohortText,
    Reading,
    Baseform,
    ReadingTag,
}

impl std::fmt::Display for BinaryField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BinaryField::Length => "window length",
            BinaryField::Body => "window body",
            BinaryField::WindowFlags => "window flags",
            BinaryField::TagTable => "tag table",
            BinaryField::Variable => "variable",
            BinaryField::WindowText => "window text",
            BinaryField::CohortCount => "cohort count",
            BinaryField::CohortFlags => "cohort flags",
            BinaryField::Wordform => "wordform",
            BinaryField::StaticTag => "static tag",
            BinaryField::Dependency => "dependency",
            BinaryField::Relation => "relation",
            BinaryField::CohortText => "cohort text",
            BinaryField::Reading => "reading",
            BinaryField::Baseform => "baseform",
            BinaryField::ReadingTag => "reading tag",
        })
    }
}

// [spec:cg3:req:robustness.binary-stream-validated]
/// What was wrong with a window of a binary input stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BinaryStreamFault {
    /// The stream, or the window's body, ends inside the field.
    #[error("the packet ends inside the {field}")]
    Truncated { field: BinaryField },
    /// A tag reference past the end of the window's tag table.
    #[error("the {field} names tag {index}, but the window's tag table holds {count}")]
    TagIndex {
        field: BinaryField,
        index: u16,
        count: usize,
    },
    /// A dependency or relation number the flat hash containers reserve as a
    /// sentinel.
    #[error("the {field} number {value:#x} is reserved")]
    ReservedNumber { field: BinaryField, value: u32 },
}

/// Which count a [`RunError::BinaryStreamOverflow`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryCount {
    Tags,
    Variables,
    Cohorts,
    StaticTags,
    Relations,
    Readings,
    ReadingTags,
    StringBytes,
    BodyBytes,
}

impl std::fmt::Display for BinaryCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BinaryCount::Tags => "distinct tags",
            BinaryCount::Variables => "variables",
            BinaryCount::Cohorts => "cohorts",
            BinaryCount::StaticTags => "static tags on one cohort",
            BinaryCount::Relations => "relations on one cohort",
            BinaryCount::Readings => "readings on one cohort",
            BinaryCount::ReadingTags => "tags on one reading",
            BinaryCount::StringBytes => "bytes in one string",
            BinaryCount::BodyBytes => "bytes in the window body",
        })
    }
}

// [spec:cg3:req:robustness.allocation-bounded]
// [spec:cg3:req:robustness.binary-stream-validated]
/// A window packet's `u32 LE` length and the body it announces.
///
/// The body is read as it arrives, so memory follows the bytes the stream
/// supplies rather than the length it declares, and a stream that ends first
/// is a truncated packet.
pub(super) fn read_window_body<R: Read>(input: &mut R, window: u32) -> Result<Vec<u8>, RunError> {
    let truncated = |field| RunError::BinaryStreamWindow {
        window,
        offset: 0,
        fault: BinaryStreamFault::Truncated { field },
    };
    let mut len = [0u8; 4];
    if let Err(e) = input.read_exact(&mut len) {
        return Err(match e.kind() {
            std::io::ErrorKind::UnexpectedEof => truncated(BinaryField::Length),
            _ => RunError::Io(e),
        });
    }
    let len = u32::from_le_bytes(len);
    let mut body = Vec::new();
    input.take(u64::from(len)).read_to_end(&mut body)?;
    if body.len() as u64 != u64::from(len) {
        return Err(truncated(BinaryField::Body));
    }
    Ok(body)
}

// [spec:cg3:req:robustness.binary-stream-validated]
/// A bounds-checked cursor over one window packet's body. A read past the end
/// and a tag index past the window's tag table are each a
/// [`RunError::BinaryStreamWindow`] naming the window and the byte the
/// offending read begins at.
pub(super) struct WindowBody<'b> {
    pub(super) bytes: &'b [u8],
    pub(super) pos: usize,
    pub(super) window: u32,
}

impl<'b> WindowBody<'b> {
    fn fault(&self, offset: usize, fault: BinaryStreamFault) -> RunError {
        RunError::BinaryStreamWindow {
            window: self.window,
            offset,
            fault,
        }
    }

    fn take(&mut self, n: usize, field: BinaryField) -> Result<&'b [u8], RunError> {
        let bytes = self.bytes.get(self.pos..).and_then(|rest| rest.get(..n));
        let Some(bytes) = bytes else {
            return Err(self.fault(self.pos, BinaryStreamFault::Truncated { field }));
        };
        self.pos += n;
        Ok(bytes)
    }

    pub(super) fn u8(&mut self, field: BinaryField) -> Result<u8, RunError> {
        let b = self.take(1, field)?;
        Ok(b[0])
    }

    pub(super) fn u16(&mut self, field: BinaryField) -> Result<u16, RunError> {
        let b = self.take(2, field)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(super) fn u32(&mut self, field: BinaryField) -> Result<u32, RunError> {
        let b = self.take(4, field)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// READ_STR: a `u16 LE` byte length, then that many UTF-8 bytes.
    pub(super) fn string(&mut self, field: BinaryField) -> Result<String, RunError> {
        let len = self.u16(field)?;
        let bytes = self.take(usize::from(len), field)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// A `u16` index into the window's tag table, resolved.
    pub(super) fn tag(&mut self, tags: &[TagId], field: BinaryField) -> Result<TagId, RunError> {
        let at = self.pos;
        let index = self.u16(field)?;
        tags.get(usize::from(index)).copied().ok_or_else(|| {
            self.fault(
                at,
                BinaryStreamFault::TagIndex {
                    field,
                    index,
                    count: tags.len(),
                },
            )
        })
    }

    // [spec:cg3:req:robustness.reserved-keys]
    /// A `u32` dependency or relation number. The two numbers the flat hash
    /// containers reserve are refused here, before any container sees them —
    /// except `allowed`, the one a field gives its own meaning (a parent of
    /// `DEP_NO_PARENT`).
    pub(super) fn number(
        &mut self,
        field: BinaryField,
        allowed: Option<u32>,
    ) -> Result<u32, RunError> {
        let at = self.pos;
        let value = self.u32(field)?;
        if value >= u32::MAX - 1 && Some(value) != allowed {
            return Err(self.fault(at, BinaryStreamFault::ReservedNumber { field, value }));
        }
        Ok(value)
    }
}

// [spec:cg3:req:robustness.checked-arithmetic]
/// One window packet as the writer builds it: the per-window tag table (C++
/// `tags_to_write` + `tag_index`) and the window's number for errors. Every
/// count and length is checked against the width the format stores it in, so
/// a window the format cannot represent is refused rather than written with a
/// wrapped count.
pub(super) struct PacketWriter {
    pub(super) tags: Vec<TagId>,
    index: HashMap<TagId, u16>,
    window: u32,
}

impl PacketWriter {
    pub(super) fn new(window: u32) -> Self {
        PacketWriter {
            tags: Vec::new(),
            index: HashMap::new(),
            window,
        }
    }

    pub(super) fn overflow(&self, what: BinaryCount, count: usize, max: usize) -> RunError {
        RunError::BinaryStreamOverflow {
            window: self.window,
            what,
            count,
            max,
        }
    }

    /// WRITE_U16_INTO for a count or length, refused past `u16::MAX`.
    pub(super) fn count(
        &self,
        buffer: &mut Vec<u8>,
        count: usize,
        what: BinaryCount,
    ) -> Result<(), RunError> {
        let n =
            u16::try_from(count).map_err(|_| self.overflow(what, count, usize::from(u16::MAX)))?;
        buffer.extend_from_slice(&n.to_le_bytes());
        Ok(())
    }

    /// WRITE_TAG_INTO: the tag's `u16` index in the window's table, which it
    /// joins if it is new.
    pub(super) fn tag(&mut self, buffer: &mut Vec<u8>, tag: TagId) -> Result<(), RunError> {
        let index = match self.index.get(&tag) {
            Some(&index) => index,
            None => {
                let next = self.tags.len();
                let index = u16::try_from(next).map_err(|_| {
                    self.overflow(BinaryCount::Tags, next + 1, usize::from(u16::MAX))
                })?;
                self.tags.push(tag);
                self.index.insert(tag, index);
                index
            }
        };
        buffer.extend_from_slice(&index.to_le_bytes());
        Ok(())
    }

    /// WRITE_STR_INTO: `[u16 LE byte-length][UTF-8 bytes]`.
    pub(super) fn string(&self, buffer: &mut Vec<u8>, s: &str) -> Result<(), RunError> {
        self.count(buffer, s.len(), BinaryCount::StringBytes)?;
        buffer.extend_from_slice(s.as_bytes());
        Ok(())
    }
}
