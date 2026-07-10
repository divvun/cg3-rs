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
//! * Errors are surfaced as `Err(String)`, the analog of the C++
//!   `throw std::runtime_error(msg)`.

use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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
    pub fn start(&mut self, cmdline: &str) -> Result<(), String> {
        // POSIX popen_plus execs `sh -c command`; mirror that (cmd /C on Windows).
        let (shell, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
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
            Err(_) => {
                // "Process could not start!\nCmdline: " + cmdline + '\n', then
                // decorated with the system error text.
                let mut msg = String::from("Process could not start!\nCmdline: ");
                msg.push_str(cmdline);
                msg.push('\n');
                Err(self.format_last_error(msg))
            }
        }
    }

    // [spec:cg3:def:process.process.read-fn]
    // [spec:cg3:sem:process.process.read-fn]
    pub fn read(&mut self, buffer: &mut [u8], count: usize) -> Result<(), String> {
        // fread(buffer, 1, count, child->read_fp) != count -> error.
        // read_exact treats a short read (EOF before `count`) as an error.
        let res = match self.stdout.as_mut() {
            Some(out) => out.read_exact(&mut buffer[..count]),
            None => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        };
        match res {
            Ok(()) => Ok(()),
            Err(_) => Err(self.format_last_error("Process.read(char*,size_t)")),
        }
    }

    // [spec:cg3:def:process.process.write-fn]
    // [spec:cg3:sem:process.process.write-fn]
    pub fn write(&mut self, buffer: &[u8], length: usize) -> Result<(), String> {
        // fwrite(buffer, 1, length, child->write_fp) != length -> error.
        let res = match self.stdin.as_mut() {
            Some(inp) => inp.write_all(&buffer[..length]),
            None => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        };
        match res {
            Ok(()) => Ok(()),
            Err(_) => Err(self.format_last_error("Process.write(char*,size_t)")),
        }
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

    // [spec:cg3:def:process.process.format-last-error-fn]
    // [spec:cg3:sem:process.process.format-last-error-fn]
    fn format_last_error(&self, msg: impl Into<String>) -> String {
        let mut msg = msg.into();
        if !msg.is_empty() {
            msg.push(' ');
        }
        // POSIX build: "strerror: " + strerror(errno), no trailing newline.
        // (The Win32 build appends "GetLastError: " + FormatMessageA + '\n'.)
        // std::io::Error::last_os_error() reads errno / GetLastError.
        msg.push_str("strerror: ");
        msg.push_str(&io::Error::last_os_error().to_string());
        msg
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

    // A failed start decorates the message via format_last_error ("strerror: ...").
    // This drives start's error path and format_last_error together.
    // [spec:cg3:sem:process.process.format-last-error-fn/test]
    #[test]
    fn start_failure_formats_error() {
        // format_last_error appends a "strerror: " suffix to any message.
        let p = Process::new();
        let formatted = p.format_last_error("boom");
        assert!(formatted.starts_with("boom "));
        assert!(formatted.contains("strerror: "));

        // read/write on an unstarted process hit the BrokenPipe error path, which
        // also routes through format_last_error.
        let mut p2 = Process::new();
        let mut buf = [0u8; 4];
        let err = p2.read(&mut buf, 4).unwrap_err();
        assert!(err.contains("strerror: "));
        let werr = p2.write(b"abcd", 4).unwrap_err();
        assert!(werr.contains("strerror: "));
    }
}
