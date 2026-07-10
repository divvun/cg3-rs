# src/streambuf.hpp

> [spec:cg3:def:streambuf.cg3.bstreambuf]
> class bstreambuf : public std::streambuf {
>   std::string buffer;
>   char_type ch = 0;
>   size_t offset = 0;
>   std::istream* stream;
> }

> [spec:cg3:def:streambuf.cg3.bstreambuf.bstreambuf-fn]
> bstreambuf(std::istream& input, std::string&& b)

> [spec:cg3:sem:streambuf.cg3.bstreambuf.bstreambuf-fn]
> Constructs a `std::streambuf` that first serves bytes from an in-memory
> prefix buffer and then falls through to an underlying istream — used to
> "un-read"/prepend a chunk (e.g. bytes already peeked) in front of a
> stream. Move-initializes member `buffer` from the rvalue string `b`, and
> stores `&input` in member `stream`. Member `offset` starts at 0 and
> `ch` at 0. Calls `setg(&ch, &ch+1, &ch+1)` — the get area is the single
> member byte `ch` with the read pointer already at the end
> (`gptr == egptr`), so the first character request triggers `underflow`.
> In the port, model this as a reader that yields `buffer[0..]` first, then
> the wrapped stream.

> [spec:cg3:def:streambuf.cg3.bstreambuf.underflow-fn]
> int_type underflow()

> [spec:cg3:sem:streambuf.cg3.bstreambuf.underflow-fn]
> Supplies the next single character when the get area is exhausted. If
> `offset < buffer.size()` (prefix buffer not yet drained), take
> `buffer[offset]` cast to the unsigned char type (so bytes 0x80–0xFF are
> not sign-extended into a negative/EOF-looking value) and increment
> `offset`; else read `c = stream->get()` from the underlying istream
> (which returns EOF at end). Store `(char_type)c` into member `ch`, call
> `setg(&ch, &ch, &ch+1)` so `ch` is now the current available character,
> and return `c` (as `int_type`). Net effect: the prefix `buffer` is
> consumed first, then the underlying stream; EOF from the stream
> propagates as the return value. Note the unsigned cast is applied to
> buffer bytes but not to `stream->get()` (which is already an EOF-aware
> `int_type`).

> [spec:cg3:def:streambuf.cg3.bstreambuf.xsgetn-fn]
> std::streamsize xsgetn(char_type* s, std::streamsize count)

> [spec:cg3:sem:streambuf.cg3.bstreambuf.xsgetn-fn]
> Bulk read of up to `count` characters into `s`, draining the prefix
> `buffer` first and then the underlying stream. Set `i = 0`. Copy loop:
> while `offset < buffer.size()` AND `i < count`, do `s[i] = buffer[offset]`,
> `++offset`, `++i`. If more are still needed (`i < count`),
> `stream->read(s + i, count - i)` then `i += stream->gcount()` (actual
> bytes read, which may be fewer than requested at EOF). Then write
> `s[i] = 0` (NUL-terminate at the number of bytes obtained). Reset the get
> area with `setg(&ch, &ch+1, &ch+1)` (empty, so a subsequent single-char
> request re-underflows and any buffered `ch` is discarded). Return `i`
> (bytes read). QUIRK/BUG: `s[i] = 0` always writes one byte past the data;
> if the request was fully satisfied (`i == count`), this writes `s[count]`,
> one byte beyond the requested length — the caller's buffer must have room
> for `count + 1`, or this overruns by one.

> [spec:cg3:def:streambuf.cg3.cstreambuf]
> class cstreambuf : public std::streambuf {
>   char_type ch = 0;
>   FILE* stream;
> }

> [spec:cg3:def:streambuf.cg3.cstreambuf.cstreambuf-fn]
> cstreambuf(FILE* s)

> [spec:cg3:sem:streambuf.cg3.cstreambuf.cstreambuf-fn]
> Constructs a `std::streambuf` adapter over a C `FILE*`. Stores `s` in
> member `stream` (member `ch` starts 0). Calls `setg(&ch, &ch+1, &ch+1)`
> — the get area is the single member byte `ch` with the read pointer at
> the end (`gptr == egptr`), so the first read request triggers
> `underflow`. In the port this is a streambuf/reader-writer wrapping an
> owned or borrowed C file handle; reads use `fgetc`/`fread`, writes use
> `fputc`/`fwrite`, and sync uses `fflush`.

> [spec:cg3:def:streambuf.cg3.cstreambuf.overflow-fn]
> int_type overflow(int_type ch = Base::traits_type::eof())

> [spec:cg3:sem:streambuf.cg3.cstreambuf.overflow-fn]
> Writes a single overflow character to the FILE. If `ch` is not the
> traits EOF value, write it with `fputc(ch, stream)` and return that
> call's result (the character written on success, or EOF on error). If
> `ch` is EOF (the default argument, e.g. a flush-type call), do nothing
> and return 0. So a genuine character is forwarded to `fputc`; an EOF
> argument is a no-op.

> [spec:cg3:def:streambuf.cg3.cstreambuf.sync-fn]
> int sync()

> [spec:cg3:sem:streambuf.cg3.cstreambuf.sync-fn]
> Flushes the wrapped FILE: returns `fflush(stream)` (0 on success, EOF on
> error). This is the streambuf `sync` override, invoked on flush.

> [spec:cg3:def:streambuf.cg3.cstreambuf.underflow-fn]
> int_type underflow()

> [spec:cg3:sem:streambuf.cg3.cstreambuf.underflow-fn]
> Supplies the next character when the get area is exhausted. Read
> `c = fgetc(stream)` (an `int`), store `(char_type)c` into member `ch`,
> call `setg(&ch, &ch, &ch+1)` so `ch` becomes the current available
> character, and return `c`. On end-of-file `fgetc` returns EOF (-1), which
> is returned directly to signal end (even though `ch` is then set to
> `(char)EOF` = 0xFF and still pointed at); the EOF int propagating as the
> return value is what stops reads.

> [spec:cg3:def:streambuf.cg3.cstreambuf.xsgetn-fn]
> std::streamsize xsgetn(char_type* s, std::streamsize count)

> [spec:cg3:sem:streambuf.cg3.cstreambuf.xsgetn-fn]
> Bulk read of `count` bytes into `s`. First `setg(&ch, &ch+1, &ch+1)`
> resets the get area to empty (`gptr == egptr`), discarding any single
> character previously buffered in `ch`. Then `return fread(s, 1, count,
> stream)` — reads up to `count` elements of size 1 from the FILE into `s`
> and returns the number actually read. QUIRK: because it resets rather
> than consuming `ch`, if `underflow` had buffered a character it is
> dropped here and the bulk read comes straight from the FILE — a
> pushed-back byte can be lost when a single-char read is followed by a
> bulk read.

> [spec:cg3:def:streambuf.cg3.cstreambuf.xsputn-fn]
> std::streamsize xsputn(const char_type* s, std::streamsize count)

> [spec:cg3:sem:streambuf.cg3.cstreambuf.xsputn-fn]
> Bulk write: `return fwrite(s, 1, count, stream)` — writes `count`
> elements of size 1 from `s` to the wrapped FILE and returns the number
> actually written. No buffering of its own.

