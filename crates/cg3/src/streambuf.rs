//! Port of `src/streambuf.hpp`.
//!
//! Two custom `std::streambuf` subclasses:
//!
//! * [`cstreambuf`] — a streambuf adapter over a C `FILE*`. Reads use
//!   `fgetc`/`fread`, writes use `fputc`/`fwrite`, sync uses `fflush`.
//! * [`bstreambuf`] — serves an in-memory prefix `buffer` first, then falls
//!   through to an underlying `std::istream` (used to "un-read"/prepend bytes
//!   already peeked in front of a stream).
//!
//! Rust has no `std::streambuf`, so the streambuf role is expressed via
//! `std::io::{Read, Write}` (the faithful analog): the wrapped `stream` is a
//! generic bounded by `Read`/`Write`, and the five virtual overrides
//! (`underflow`/`overflow`/`xsgetn`/`xsputn`/`sync`) are ported 1:1 as inherent
//! methods. The C++ `setg(...)` get-area juggling over the single member byte
//! `ch` is modelled by the `avail` flag (`avail == (gptr < egptr)`): whether
//! `ch` currently holds a not-yet-consumed character.
//!
//! Flagged quirks are reproduced faithfully:
//! * `bstreambuf::xsgetn` always writes `s[i] = 0` one byte past the data; when
//!   the request is fully satisfied (`i == count`) this is `s[count]`, one past
//!   the requested length — in Rust this PANICS unless the caller supplied a
//!   slice with `count + 1` room (the same contract the C++ silently relies on,
//!   where it is instead an out-of-bounds overrun / UB).
//! * `cstreambuf::xsgetn` resets the get area rather than consuming `ch`, so a
//!   byte buffered by a prior `underflow` (a pushed-back byte) is dropped and
//!   the bulk read comes straight from the FILE.
//!
//! `std::string` (byte buffer) maps to `Vec<u8>`; `char` buffers map to `[u8]`.

use std::io::{self, Read, Write};

/// C++ `Base::char_type` (`char`) — a raw byte in a stream buffer.
type char_type = u8;
/// C++ `Base::int_type` (`int`) — an EOF-aware character value (`-1` == EOF).
type int_type = i32;
/// C++ `std::streamsize` — a signed byte count.
type streamsize = i64;

/// C++ `Base::traits_type::eof()`.
const EOF: int_type = -1;

// --- Helpers modelling the C stdio calls the C++ streambufs delegate to. ---

/// `fgetc(stream)` / `istream::get()`: read one byte, or `EOF` at end/error.
fn fgetc<R: Read>(r: &mut R) -> int_type {
    let mut b = [0u8; 1];
    loop {
        match r.read(&mut b) {
            Ok(0) => return EOF,
            Ok(_) => return b[0] as int_type,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return EOF,
        }
    }
}

/// `fread(buf, 1, buf.len(), stream)` / `istream::read` + `gcount()`: read up to
/// `buf.len()` bytes, returning the number actually read (short only at EOF).
fn fread<R: Read>(r: &mut R, buf: &mut [u8]) -> streamsize {
    let mut total = 0usize;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    total as streamsize
}

/// `fputc(c, stream)`: write the low byte of `c`; return the written byte value
/// (`(unsigned char)c` promoted to int) on success, or `EOF` on error.
fn fputc<W: Write>(w: &mut W, c: int_type) -> int_type {
    let b = [c as u8];
    match w.write_all(&b) {
        Ok(()) => (c as u8) as int_type,
        Err(_) => EOF,
    }
}

/// `fwrite(buf, 1, buf.len(), stream)`: write bytes, returning the number
/// actually written (fewer than `buf.len()` on error).
fn fwrite<W: Write>(w: &mut W, buf: &[u8]) -> streamsize {
    let mut total = 0usize;
    while total < buf.len() {
        match w.write(&buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    total as streamsize
}

/// `fflush(stream)`: 0 on success, `EOF` on error.
fn fflush<W: Write>(w: &mut W) -> i32 {
    match w.flush() {
        Ok(()) => 0,
        Err(_) => EOF,
    }
}

// [spec:cg3:def:streambuf.cg3.cstreambuf]
/// Streambuf adapter over a C `FILE*` — here a generic stream `S` (bounded by
/// `Read` for the get side, `Write` for the put side).
pub struct cstreambuf<S> {
    ch: char_type,
    stream: S,
    /// Models `setg` state over the single byte `ch`: `avail == (gptr < egptr)`,
    /// i.e. whether `ch` currently holds a not-yet-consumed character.
    avail: bool,
}

impl<S> cstreambuf<S> {
    // [spec:cg3:def:streambuf.cg3.cstreambuf.cstreambuf-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.cstreambuf-fn]
    pub fn new(s: S) -> Self {
        // setg(&ch, &ch+1, &ch+1): gptr == egptr -> empty get area, so the first
        // read request triggers underflow.
        cstreambuf { ch: 0, stream: s, avail: false }
    }
}

impl<S: Read> cstreambuf<S> {
    // Get
    // [spec:cg3:def:streambuf.cg3.cstreambuf.underflow-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.underflow-fn]
    pub fn underflow(&mut self) -> int_type {
        // auto c = fgetc(stream);
        let c = fgetc(&mut self.stream);
        // ch = static_cast<char_type>(c);  (EOF -> (char)-1 == 0xFF)
        self.ch = c as char_type;
        // setg(&ch, &ch, &ch+1): `ch` becomes the current available character.
        self.avail = true;
        // return c;  (EOF propagates as the return value to stop reads)
        c
    }

    // [spec:cg3:def:streambuf.cg3.cstreambuf.xsgetn-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.xsgetn-fn]
    pub fn xsgetn(&mut self, s: &mut [u8], count: streamsize) -> streamsize {
        // setg(&ch, &ch+1, &ch+1): reset the get area to empty, discarding any
        // char previously buffered in `ch`.
        // QUIRK: because this resets rather than consuming `ch`, a byte pushed
        // back by `underflow` is dropped here — the bulk read comes straight
        // from the FILE.
        self.avail = false;
        // return fread(s, 1, count, stream);
        fread(&mut self.stream, &mut s[..count as usize])
    }
}

impl<S: Write> cstreambuf<S> {
    // Put
    // [spec:cg3:def:streambuf.cg3.cstreambuf.overflow-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.overflow-fn]
    //
    // The C++ default argument is `ch = traits::eof()` (a flush-type call); pass
    // `EOF` explicitly for that no-op case.
    pub fn overflow(&mut self, ch: int_type) -> int_type {
        if ch != EOF {
            // return fputc(ch, stream);
            return fputc(&mut self.stream, ch);
        }
        0
    }

    // [spec:cg3:def:streambuf.cg3.cstreambuf.xsputn-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.xsputn-fn]
    pub fn xsputn(&mut self, s: &[u8], count: streamsize) -> streamsize {
        // return fwrite(s, 1, count, stream);
        fwrite(&mut self.stream, &s[..count as usize])
    }

    // [spec:cg3:def:streambuf.cg3.cstreambuf.sync-fn]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.sync-fn]
    pub fn sync(&mut self) -> i32 {
        // return fflush(stream);
        fflush(&mut self.stream)
    }
}

// [spec:cg3:def:streambuf.cg3.bstreambuf]
/// Serves an in-memory prefix `buffer` first, then the underlying stream `R`
/// (`std::istream`; `Read` is a sufficient analog for its `get`/`read`).
pub struct bstreambuf<R> {
    buffer: Vec<u8>,
    ch: char_type,
    offset: usize,
    stream: R,
    /// Models `setg` state over the single byte `ch` (see [`cstreambuf`]).
    avail: bool,
}

impl<R> bstreambuf<R> {
    // [spec:cg3:def:streambuf.cg3.bstreambuf.bstreambuf-fn]
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.bstreambuf-fn]
    pub fn new(input: R, b: Vec<u8>) -> Self {
        // buffer(std::move(b)), stream(&input); offset 0, ch 0.
        // setg(&ch, &ch+1, &ch+1): gptr == egptr -> first request underflows.
        bstreambuf {
            buffer: b,
            ch: 0,
            offset: 0,
            stream: input,
            avail: false,
        }
    }
}

impl<R: Read> bstreambuf<R> {
    // [spec:cg3:def:streambuf.cg3.bstreambuf.underflow-fn]
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.underflow-fn]
    pub fn underflow(&mut self) -> int_type {
        let c: int_type;
        if self.offset < self.buffer.len() {
            // c = static_cast<make_unsigned<char_type>>(buffer[offset++]);
            // u8 -> i32 is the unsigned widening: bytes 0x80..0xFF stay 128..255.
            c = self.buffer[self.offset] as int_type;
            self.offset += 1;
        } else {
            // c = stream->get();  (next byte, or EOF at end — no unsigned cast)
            c = fgetc(&mut self.stream);
        }
        // ch = static_cast<char_type>(c);
        self.ch = c as char_type;
        // setg(&ch, &ch, &ch+1): `ch` becomes the current available character.
        self.avail = true;
        c
    }

    // [spec:cg3:def:streambuf.cg3.bstreambuf.xsgetn-fn]
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.xsgetn-fn]
    pub fn xsgetn(&mut self, s: &mut [u8], count: streamsize) -> streamsize {
        let mut i: streamsize = 0;
        // Drain the prefix buffer first.
        while self.offset < self.buffer.len() && i < count {
            s[i as usize] = self.buffer[self.offset];
            self.offset += 1;
            i += 1;
        }
        // Then read the remainder from the underlying stream.
        if i < count {
            // stream->read(s + i, count - i); i += stream->gcount();
            let start = i as usize;
            let end = count as usize;
            let got = fread(&mut self.stream, &mut s[start..end]);
            i += got;
        }
        // s[i] = 0;  NUL-terminate at the number of bytes obtained.
        // QUIRK/BUG: this always writes one byte past the data. When the request
        // was fully satisfied (`i == count`) it writes `s[count]`, one past the
        // requested length. The C++ overruns the caller's buffer by one (UB
        // unless they reserved `count + 1`); in Rust this indexes `s[count]` and
        // PANICS unless the caller provided the same `count + 1` room.
        s[i as usize] = 0;
        // setg(&ch, &ch+1, &ch+1): reset get area to empty (discards any `ch`).
        self.avail = false;
        // return i;
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // cstreambuf get side: ctor starts with an empty get area; underflow reads one
    // byte (returning EOF at end); xsgetn bulk-reads straight from the stream,
    // discarding any byte previously buffered by underflow (the documented quirk).
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.cstreambuf-fn/test]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.underflow-fn/test]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.xsgetn-fn/test]
    #[test]
    fn cstreambuf_read_side() {
        // ctor: empty get area.
        let mut sb = cstreambuf::new(Cursor::new(b"abc".to_vec()));
        assert!(!sb.avail);

        // underflow reads bytes one at a time; EOF is -1 at end.
        assert_eq!(sb.underflow(), b'a' as int_type);
        assert!(sb.avail);
        assert_eq!(sb.underflow(), b'b' as int_type);

        // xsgetn resets the get area (drops the 'b' buffered in `ch`) and bulk
        // reads straight from the FILE, so it continues from 'c'.
        let mut buf = [0u8; 4];
        let got = sb.xsgetn(&mut buf, 2);
        assert!(!sb.avail); // get area reset
        assert_eq!(got, 1); // only 'c' remains
        assert_eq!(&buf[..1], b"c");

        // underflow at true end returns EOF.
        assert_eq!(sb.underflow(), EOF);
    }

    // cstreambuf put side: overflow writes one byte (and is a no-op flush call
    // when given EOF); xsputn bulk-writes; sync flushes the sink.
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.overflow-fn/test]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.xsputn-fn/test]
    // [spec:cg3:sem:streambuf.cg3.cstreambuf.sync-fn/test]
    #[test]
    fn cstreambuf_write_side() {
        let mut sb = cstreambuf::new(Vec::<u8>::new());

        // overflow(EOF) is a flush-type no-op returning 0 (writes nothing).
        assert_eq!(sb.overflow(EOF), 0);
        // overflow of a real char writes it and echoes the byte value.
        assert_eq!(sb.overflow(b'H' as int_type), b'H' as int_type);

        // xsputn bulk-writes `count` bytes and returns the count written.
        assert_eq!(sb.xsputn(b"ello!", 5), 5);

        // sync flushes (0 == success).
        assert_eq!(sb.sync(), 0);

        // The wrapped Vec now holds everything written.
        assert_eq!(sb.stream, b"Hello!");
    }

    // bstreambuf serves the in-memory prefix first, then falls through to the
    // underlying stream. underflow walks the prefix then the stream; xsgetn drains
    // the prefix then reads the remainder, and NUL-terminates at s[i] (the quirk:
    // requires count+1 room, exercised with an oversized slice).
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.bstreambuf-fn/test]
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.underflow-fn/test]
    // [spec:cg3:sem:streambuf.cg3.bstreambuf.xsgetn-fn/test]
    #[test]
    fn bstreambuf_prefix_then_stream() {
        // ctor: prefix "AB" in front of a stream carrying "cd".
        let mut sb = bstreambuf::new(Cursor::new(b"cd".to_vec()), b"AB".to_vec());
        assert!(!sb.avail);

        // underflow serves the prefix first (unsigned-widened), then the stream.
        assert_eq!(sb.underflow(), b'A' as int_type);
        assert!(sb.avail);
        assert_eq!(sb.underflow(), b'B' as int_type);
        assert_eq!(sb.underflow(), b'c' as int_type); // fell through to stream
        assert_eq!(sb.underflow(), b'd' as int_type);
        assert_eq!(sb.underflow(), EOF);

        // Fresh instance for xsgetn: prefix "AB" + stream "cd".
        let mut sb2 = bstreambuf::new(Cursor::new(b"cd".to_vec()), b"AB".to_vec());
        // count+1 room for the always-one-past NUL write (the documented quirk).
        let mut buf = [0u8; 5];
        let got = sb2.xsgetn(&mut buf, 4);
        assert_eq!(got, 4);
        assert_eq!(&buf[..4], b"ABcd"); // prefix drained, then stream
        assert_eq!(buf[4], 0); // NUL written one past the data (s[count])
        assert!(!sb2.avail); // get area reset
    }
}
