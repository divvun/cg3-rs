//! Reading back an `EXTERNAL` process's reply to a window.
//!
//! The reply is untrusted input: every read is checked for a short or failed
//! read, every count against the window that was sent, and each reading's
//! length-prefixed packet is read as it arrives, so memory follows the bytes
//! the process delivers rather than the length it declares. The wire format is
//! the C++ one (host-order `u32`s, `u16`-length-prefixed strings); only the
//! checking is new.

use crate::cohort::DEP_NO_PARENT;
use crate::error::RunError;
use crate::process::{Process, ProcessError};
use crate::types::GlobalNumber;

/// Which part of an `EXTERNAL` reply an [`ExternalFault`] concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalField {
    /// The window's length, number and cohort count.
    WindowHeader,
    /// A cohort's length, number, flags and parent.
    CohortHeader,
    Wordform,
    ReadingCount,
    /// A reading's length prefix and the packet it announces.
    Reading,
    ReadingFlags,
    Baseform,
    /// A reading's tag count and tags.
    Tags,
    CohortText,
}

impl std::fmt::Display for ExternalField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ExternalField::WindowHeader => "window header",
            ExternalField::CohortHeader => "cohort header",
            ExternalField::Wordform => "wordform",
            ExternalField::ReadingCount => "reading count",
            ExternalField::Reading => "reading packet",
            ExternalField::ReadingFlags => "reading flags",
            ExternalField::Baseform => "baseform",
            ExternalField::Tags => "tags",
            ExternalField::CohortText => "cohort text",
        })
    }
}

// [spec:cg3:req:robustness.external-validated]
/// What was wrong with an `EXTERNAL` reply.
#[derive(Debug, thiserror::Error)]
pub enum ExternalFault {
    /// The process's output ended, or could not be read, part-way through.
    #[error("breaks off in its {field}: {source}")]
    Truncated {
        field: ExternalField,
        #[source]
        source: ProcessError,
    },
    /// More cohorts than the window that was sent holds.
    #[error("has {got} cohorts, more than the {sent} sent")]
    Cohorts { sent: usize, got: u32 },
    /// More readings for a cohort than were sent for it.
    #[error("has {got} readings for cohort {cohort}, more than the {sent} sent")]
    Readings { cohort: u32, sent: usize, got: u32 },
    /// A reading's fields run past the length its packet declared.
    #[error("overruns the {len}-byte reading packet of cohort {cohort} in its {field}")]
    ReadingOverrun {
        cohort: u32,
        len: usize,
        field: ExternalField,
    },
    /// A parent number the flat hash containers reserve as a sentinel.
    #[error("gives cohort {cohort} the reserved parent number {parent:#x}")]
    ReservedParent { cohort: u32, parent: u32 },
}

/// An `EXTERNAL` reply being read back: the process, and the number of the
/// window the reply is for, which every failure names.
pub struct Reply<'p> {
    input: &'p mut Process,
    window: u32,
}

impl<'p> Reply<'p> {
    pub(crate) fn new(input: &'p mut Process, window: u32) -> Self {
        Reply { input, window }
    }

    /// The run error for `fault` in this reply.
    pub(crate) fn fault(&self, fault: ExternalFault) -> RunError {
        RunError::ExternalReply {
            window: self.window,
            fault,
        }
    }

    /// Fill `buf` from the process; a short or failed read is a reply that
    /// breaks off in `field`.
    fn fill(&mut self, buf: &mut [u8], field: ExternalField) -> Result<(), RunError> {
        let count = buf.len();
        match self.input.read(buf, count) {
            Ok(()) => Ok(()),
            Err(source) => Err(self.fault(ExternalFault::Truncated { field, source })),
        }
    }

    /// One host-order `u32` (C++ `readRaw`).
    pub(crate) fn u32(&mut self, field: ExternalField) -> Result<u32, RunError> {
        let mut bytes = [0u8; 4];
        self.fill(&mut bytes, field)?;
        Ok(u32::from_ne_bytes(bytes))
    }

    /// A host-order `u16` byte length and that many UTF-8 bytes (C++
    /// `readUTF8_Raw`). The prefix's width bounds what it can ask for.
    pub(crate) fn string(&mut self, field: ExternalField) -> Result<String, RunError> {
        let mut len = [0u8; 2];
        self.fill(&mut len, field)?;
        let mut bytes = vec![0u8; usize::from(u16::from_ne_bytes(len))];
        self.fill(&mut bytes, field)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A `u32` count, refused when it is more than the `sent` the window
    /// carried out, with the fault `refuse` builds from the count received.
    pub(crate) fn count(
        &mut self,
        field: ExternalField,
        sent: usize,
        refuse: impl FnOnce(u32) -> ExternalFault,
    ) -> Result<usize, RunError> {
        let got = self.u32(field)?;
        match usize::try_from(got) {
            Ok(n) if n <= sent => Ok(n),
            _ => Err(self.fault(refuse(got))),
        }
    }

    // [spec:cg3:req:robustness.reserved-keys]
    /// A cohort's parent number: `DEP_NO_PARENT` means none, and the other
    /// number the flat hash containers reserve is refused before any
    /// container sees it.
    pub(crate) fn parent(&mut self, cohort: u32) -> Result<Option<GlobalNumber>, RunError> {
        match self.u32(ExternalField::CohortHeader)? {
            DEP_NO_PARENT => Ok(None),
            parent if parent == DEP_NO_PARENT - 1 => {
                Err(self.fault(ExternalFault::ReservedParent { cohort, parent }))
            }
            parent => Ok(Some(GlobalNumber(parent))),
        }
    }

    // [spec:cg3:req:robustness.allocation-bounded]
    /// A `u32`-length-prefixed reading packet for the cohort numbered
    /// `cohort`. It is read in bounded chunks, so a declared length the
    /// process never delivers costs nothing but the read that finds it short.
    pub(crate) fn reading(&mut self, cohort: u32) -> Result<ReadingPacket, RunError> {
        let len = self.u32(ExternalField::Reading)? as usize;
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        while bytes.len() < len {
            let n = (len - bytes.len()).min(chunk.len());
            self.fill(&mut chunk[..n], ExternalField::Reading)?;
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(ReadingPacket {
            bytes,
            pos: 0,
            window: self.window,
            cohort,
        })
    }
}

/// One reading's packet, read field by field. A field past the packet's end
/// is an overrun of the length the process declared for it.
pub struct ReadingPacket {
    bytes: Vec<u8>,
    pos: usize,
    window: u32,
    cohort: u32,
}

impl ReadingPacket {
    fn take(&mut self, n: usize, field: ExternalField) -> Result<&[u8], RunError> {
        let Some(bytes) = self.bytes.get(self.pos..).and_then(|rest| rest.get(..n)) else {
            return Err(RunError::ExternalReply {
                window: self.window,
                fault: ExternalFault::ReadingOverrun {
                    cohort: self.cohort,
                    len: self.bytes.len(),
                    field,
                },
            });
        };
        self.pos += n;
        Ok(bytes)
    }

    pub(crate) fn u32(&mut self, field: ExternalField) -> Result<u32, RunError> {
        let b = self.take(4, field)?;
        Ok(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn string(&mut self, field: ExternalField) -> Result<String, RunError> {
        let b = self.take(2, field)?;
        let len = u16::from_ne_bytes([b[0], b[1]]);
        let bytes = self.take(usize::from(len), field)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}
