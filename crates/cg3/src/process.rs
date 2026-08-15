//! Port of `src/process.hpp`.
//!
//! A subprocess wrapper that spawns a child with its stdin and (merged)
//! stdout/stderr connected to pipes owned by this `Process`.
//!
//! ## Platform-abstraction collapse
//!
//! The C++ header is fully `#ifdef`-split into a Win32 build (four `HANDLE`
//! pipe endpoints, `CreatePipe`/`CreateProcessA`/`ReadFile`/`WriteFile`,
//! `GetLastError`/`FormatMessageA`) and a POSIX build (a single
//! `popen_plus_process*` child, `fread`/`fwrite`/`fflush`, `strerror(errno)`).
//! Both are collapsed here onto one portable `std::process` implementation:
//! an `Option<Child>` plus its piped `ChildStdin`/`ChildStdout` (all unset at
//! construction, matching both builds' null-initialised members).
//!
//! Deviations forced by the collapse, noted for parity review:
//! * `start` runs the command through the platform shell (`sh -c` on POSIX,
//!   matching `popen_plus`; `cmd /C` on Windows) and merges the child's stderr
//!   into its stdout via a shell `2>&1` redirection, because `std::process` has
//!   no portable equivalent of the Win32 build's shared `g_hChildStd_OUT_Wr`
//!   handle (nor a way to dup a pipe write end before spawn). The Win32
//!   `CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS` creation flags have no
//!   portable `std::process` analog and are dropped.
//! * `read`/`write` use `read_exact`/`write_all` — the POSIX `fread`/`fwrite`
//!   all-or-nothing semantics (a short read/write, including EOF before the
//!   count, is an error).
//! * Errors are surfaced as [`ProcessError`], this layer's own error type per
//!   `[dec:cg3:layered-error-types]`. The C++ threw `std::runtime_error` over a
//!   string built at the failure site; here the operation and the `io::Error`
//!   that caused it travel by value and the prose lives in `Display`.

use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Which `Process` operation the OS refused.
///
/// Carries what the C++ spelled into the message at the failure site, so the
/// rendered text is unchanged while the parts stay separable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOp {
    /// `start`, with the command line that would not run.
    Start {
        cmdline: String,
    },
    Read,
    Write,
}

impl std::fmt::Display for ProcessOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // C++ `"Process could not start!\nCmdline: " + cmdline + '\n'`.
            ProcessOp::Start { cmdline } => {
                write!(f, "Process could not start!\nCmdline: {cmdline}\n")
            }
            ProcessOp::Read => f.write_str("Process.read(char*,size_t)"),
            ProcessOp::Write => f.write_str("Process.write(char*,size_t)"),
        }
    }
}

// [spec:cg3:def:process.process.format-last-error-fn+1]
// [spec:cg3:sem:process.process.format-last-error-fn+1]
/// A subprocess operation that failed, with the cause it failed with.
///
/// This is where the C++ `formatLastError` went. That function built its string
/// at the failure site by reading `errno` / `GetLastError()` back out of thread
/// state *after* the call had already returned, then threw the result; the
/// decoration lives in [`Display`](std::fmt::Display) here instead, and the OS
/// error travels in the value. The difference is not cosmetic — a consumer can
/// match on [`io::ErrorKind`] rather than parse prose, which is what
/// `[spec:cg3:req:errors.context]` asks for, and the error reported is the one
/// the failing call actually produced rather than whatever was last written to
/// `errno`.
#[derive(Debug, thiserror::Error)]
#[error("{op} strerror: {source}")]
pub struct ProcessError {
    pub op: ProcessOp,
    #[source]
    pub source: io::Error,
}

// [spec:cg3:def:process.process]
/// Owns the parent-side endpoints of the two pipes connecting to a child's
/// stdin and (merged) stdout/stderr. Collapses the Win32 four-`HANDLE` layout
/// and the POSIX single-`child` layout into an `Option<Child>` plus its piped
/// stdin/stdout.
pub struct Process {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

impl Process {
    // [spec:cg3:def:process.process.process-fn]
    // [spec:cg3:sem:process.process.process-fn]
    pub fn new() -> Self {
        // No subprocess spawned yet; all endpoints unset (null-initialised).
        Process {
            child: None,
            stdin: None,
            stdout: None,
        }
    }

    // [spec:cg3:def:process.process.start-fn]
    // [spec:cg3:sem:process.process.start-fn]
    pub fn start(&mut self, cmdline: &str) -> Result<(), ProcessError> {
        // POSIX popen_plus execs `sh -c command`; mirror that (cmd /C on Windows).
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        // Merge child stderr into stdout (`2>&1`): the portable stand-in for the
        // Win32 build pointing both hStdError and hStdOutput at one pipe handle.
        let merged = format!("{cmdline} 2>&1");

        let spawned = Command::new(shell)
            .arg(flag)
            .arg(&merged)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();

        match spawned {
            Ok(mut child) => {
                self.stdin = child.stdin.take();
                self.stdout = child.stdout.take();
                self.child = Some(child);
                Ok(())
            }
            Err(source) => Err(ProcessError {
                op: ProcessOp::Start {
                    cmdline: cmdline.to_string(),
                },
                source,
            }),
        }
    }

    // [spec:cg3:def:process.process.read-fn]
    // [spec:cg3:sem:process.process.read-fn]
    pub fn read(&mut self, buffer: &mut [u8], count: usize) -> Result<(), ProcessError> {
        // fread(buffer, 1, count, child->read_fp) != count -> error.
        // read_exact treats a short read (EOF before `count`) as an error.
        let res = match self.stdout.as_mut() {
            Some(out) => out.read_exact(&mut buffer[..count]),
            None => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        };
        res.map_err(|source| ProcessError {
            op: ProcessOp::Read,
            source,
        })
    }

    // [spec:cg3:def:process.process.write-fn]
    // [spec:cg3:sem:process.process.write-fn]
    pub fn write(&mut self, buffer: &[u8], length: usize) -> Result<(), ProcessError> {
        // fwrite(buffer, 1, length, child->write_fp) != length -> error.
        let res = match self.stdin.as_mut() {
            Some(inp) => inp.write_all(&buffer[..length]),
            None => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        };
        res.map_err(|source| ProcessError {
            op: ProcessOp::Write,
            source,
        })
    }

    // [spec:cg3:def:process.process.flush-fn]
    // [spec:cg3:sem:process.process.flush-fn]
    pub fn flush(&mut self) {
        // POSIX: fflush(child->write_fp). (The Win32 build's flush is a no-op,
        // since its write uses WriteFile directly with no buffering.)
        if let Some(inp) = self.stdin.as_mut() {
            let _ = inp.flush();
        }
    }
}

impl Default for Process {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // POSIX ~Process: popen_plus_kill (kill -9) then popen_plus_close
        // (waitpid + close). Win32 ~Process closes all four handles.
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // new() constructs a Process with no child and all endpoints unset.
    // [spec:cg3:sem:process.process.process-fn/test]
    #[test]
    fn new_is_unstarted() {
        let p = Process::new();
        assert!(p.child.is_none());
        assert!(p.stdin.is_none());
        assert!(p.stdout.is_none());
    }

    // Round-trip through a real child: start `cat` (echoes stdin to stdout), write
    // bytes, flush, and read the same bytes back. Drives start/write/flush/read.
    // [spec:cg3:sem:process.process.start-fn/test]
    // [spec:cg3:sem:process.process.write-fn/test]
    // [spec:cg3:sem:process.process.flush-fn/test]
    // [spec:cg3:sem:process.process.read-fn/test]
    #[cfg(unix)]
    #[test]
    fn cat_round_trip() {
        let mut p = Process::new();
        // `cat` copies its stdin to stdout verbatim.
        p.start("cat").expect("cat should start");
        assert!(p.child.is_some());
        assert!(p.stdin.is_some());
        assert!(p.stdout.is_some());

        let payload = b"hello pipe\n";
        p.write(payload, payload.len()).expect("write ok");
        p.flush();

        let mut buf = vec![0u8; payload.len()];
        p.read(&mut buf, payload.len()).expect("read ok");
        assert_eq!(&buf[..], payload);
        // Dropping `p` kills+reaps the child.
    }

    // The C++ `formatLastError` decoration survives, in `Display` rather than at
    // the failure site — and the OS error is reachable underneath it.
    // [spec:cg3:sem:process.process.format-last-error-fn+1/test]
    #[test]
    fn failures_carry_the_operation_and_the_os_error() {
        // read/write on an unstarted process hit the BrokenPipe error path.
        let mut p = Process::new();
        let mut buf = [0u8; 4];

        let err = p.read(&mut buf, 4).unwrap_err();
        assert_eq!(err.op, ProcessOp::Read);
        assert_eq!(err.source.kind(), io::ErrorKind::BrokenPipe);
        assert!(
            err.to_string()
                .starts_with("Process.read(char*,size_t) strerror: "),
            "{err}"
        );

        let werr = p.write(b"abcd", 4).unwrap_err();
        assert_eq!(werr.op, ProcessOp::Write);
        assert_eq!(werr.source.kind(), io::ErrorKind::BrokenPipe);
        assert!(
            werr.to_string()
                .starts_with("Process.write(char*,size_t) strerror: "),
            "{werr}"
        );

        // The cause is inspectable through `source()`, not only as text.
        let dyn_err: &dyn std::error::Error = &werr;
        assert!(dyn_err.source().is_some());
    }

    // `start`'s message keeps the C++ wording, cmdline included.
    // [spec:cg3:sem:process.process.start-fn/test]
    #[test]
    fn start_failure_names_the_cmdline() {
        let rendered = ProcessError {
            op: ProcessOp::Start {
                cmdline: "no-such-cmd".to_string(),
            },
            source: io::Error::from(io::ErrorKind::NotFound),
        }
        .to_string();
        assert!(rendered.starts_with("Process could not start!\nCmdline: no-such-cmd\n"));
        assert!(rendered.contains("strerror: "));
    }
}
