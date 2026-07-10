# src/process.hpp

> [spec:cg3:def:process.process]
> class Process {
>   HANDLE g_hChildStd_IN_Rd;
>   HANDLE g_hChildStd_IN_Wr;
>   HANDLE g_hChildStd_OUT_Rd;
>   HANDLE g_hChildStd_OUT_Wr;
> }

> [spec:cg3:def:process.process.flush-fn]
> void flush()

> [spec:cg3:sem:process.process.flush-fn]
> Flushes buffered data written to the child's stdin so it actually
> reaches the child. This marker sits on the POSIX build, whose body is
> `fflush(child->write_fp)` (flush the C stream feeding the child's
> stdin). Semantically: ensure any bytes handed to `write` are pushed to
> the subprocess. Platform note: the Windows build's `flush()` is an empty
> no-op because its `write` uses `WriteFile` directly with no buffering.
> A `std::process`-based Rust port would flush the child stdin pipe here.

> [spec:cg3:def:process.process.format-last-error-fn]
> std::string formatLastError(std::string msg = "")

> [spec:cg3:sem:process.process.format-last-error-fn]
> Builds a human-readable error string by appending the platform's
> last-system-error text to an optional caller message `msg`. If `msg` is
> non-empty, append a single space. Then (Windows build, where this marker
> sits) retrieve the description of the OS error code from `GetLastError()`
> via `FormatMessageA` (FROM_SYSTEM | ALLOCATE_BUFFER, neutral language)
> which allocates the message string; append `"GetLastError: "`, that
> message, and a newline; free the allocated buffer with `LocalFree`.
> Return the composed string. Platform note: the POSIX build instead
> appends `"strerror: "` followed by `strerror(errno)` (no trailing
> newline). Semantically: decorate a message with the current system error
> description. A Rust port would append `std::io::Error::last_os_error()`
> text.

> [spec:cg3:def:process.process.process-fn]
> Process()

> [spec:cg3:sem:process.process.process-fn]
> Default-constructs a `Process`. On the Windows build (where this marker
> sits) it initializes all four pipe handle members
> (`g_hChildStd_IN_Rd`, `g_hChildStd_IN_Wr`, `g_hChildStd_OUT_Rd`,
> `g_hChildStd_OUT_Wr`) to null; no subprocess is spawned yet. (The POSIX
> build's constructor initializes its single `child` pointer to null.)
> Semantically: a `Process` owns the endpoints of two pipes that will
> connect the parent to a child's stdin and stdout; at construction none
> exist. The destructor releases them (Windows closes all four handles;
> POSIX kills and closes the child if one was started). A Rust port holds
> an `Option<Child>` plus its piped stdin/stdout, all `None`/unset here.

> [spec:cg3:def:process.process.read-fn]
> void read(char *buffer, size_t count)

> [spec:cg3:sem:process.process.read-fn]
> Reads EXACTLY `count` bytes from the child's stdout into `buffer`
> (all-or-nothing, blocking). Windows build (this marker): `ReadFile` from
> the parent's read end of the OUT pipe (`g_hChildStd_OUT_Rd`) requesting
> `count` bytes into `bytes_read`; if `ReadFile` fails OR
> `bytes_read != count`, throw `std::runtime_error` with a
> `formatLastError("Process.read(char*,size_t)")` message. POSIX build:
> `fread(buffer, 1, count, child->read_fp)`; if the result `!= count`,
> throw the same kind of error. Semantically: pull exactly `count` bytes
> of the subprocess's output, treating a short read (including EOF before
> `count`) as an error. A Rust port would `read_exact` on the child's
> stdout and map failure to an error.

> [spec:cg3:def:process.process.start-fn]
> void start(const std::string& cmdline)

> [spec:cg3:sem:process.process.start-fn]
> Spawns a child process running the shell/command line `cmdline` with its
> stdin and stdout/stderr connected to pipes owned by this `Process`.
> Windows build (this marker) semantics, in order:
> - Create the OUT pipe (child stdout -> parent read), with inheritable
>   security attributes, storing read end `g_hChildStd_OUT_Rd` (parent) and
>   write end `g_hChildStd_OUT_Wr` (child); mark the parent read end
>   non-inheritable.
> - Create the IN pipe (parent write -> child stdin), storing read end
>   `g_hChildStd_IN_Rd` (child) and write end `g_hChildStd_IN_Wr` (parent);
>   mark the parent write end non-inheritable.
> - Configure child startup so the child's stdout AND stderr both go to
>   `g_hChildStd_OUT_Wr` (child stderr is MERGED into the same pipe as
>   stdout) and the child's stdin comes from `g_hChildStd_IN_Rd`; request
>   use of these std handles.
> - Launch the child (`CreateProcessA`) with handle inheritance enabled and
>   flags `CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS` (no console
>   window; lower-than-normal scheduling priority).
> - Each failed step throws `std::runtime_error` built via
>   `formatLastError` with a step label; a launch failure includes the full
>   `cmdline`. On success, close the returned process and thread handles
>   (the parent does not need them).
> POSIX build: `child = popen_plus(cmdline.data())` opens a bidirectional
> pipe to the command; if it returns null, throw an error that includes the
> cmdline. Semantically identical intent: fork/exec the command with piped
> stdin and merged stdout+stderr. A Rust port would use
> `std::process::Command` with `Stdio::piped()` for stdin and stdout,
> merging stderr into stdout.

> [spec:cg3:def:process.process.write-fn]
> void write(const char *buffer, size_t length)

> [spec:cg3:sem:process.process.write-fn]
> Writes EXACTLY `length` bytes from `buffer` to the child's stdin
> (all-or-nothing). Windows build (this marker): `WriteFile` to the
> parent's write end of the IN pipe (`g_hChildStd_IN_Wr`) requesting
> `length` bytes into `bytes`; if `WriteFile` fails OR `bytes != length`,
> throw `std::runtime_error` with a
> `formatLastError("Process.write(char*,size_t)")` message. POSIX build:
> `fwrite(buffer, 1, length, child->write_fp)`; if the result `!= length`,
> throw the same kind of error. Semantically: push exactly `length` bytes
> to the subprocess's input, treating a short write as an error. A Rust
> port would `write_all` to the child's stdin.

