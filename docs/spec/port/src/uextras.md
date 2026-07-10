# src/uextras.cpp, src/uextras.hpp

> [spec:cg3:def:uextras.basename-fn]
> inline const char *basename(const char *path)

> [spec:cg3:sem:uextras.basename-fn]
> Windows-only fallback (compiled only under `_WIN32`) that mimics POSIX
> `basename` on a NUL-terminated C string. If `path` is null, return the
> static string `"."`. Otherwise find the last `'\\'` and the last `'/'`
> via `strrchr` and take the greater of the two pointers with `std::max`
> (`strrchr` returns null when the char is absent, and null compares as
> the smallest pointer value, so `max` picks whichever separator was
> found, or the one nearer the end of the string if both occur). If a
> separator was found (`pos != nullptr`): if the character right after it
> (`pos[1]`) is not the NUL terminator, return `pos + 1` (pointer to the
> first character after the separator); otherwise (the separator is the
> final character) return `pos` (pointer to the separator itself). If no
> separator was found, return `path` unchanged (the source comment notes
> this is "probably non-conformant"). The returned pointer aliases into
> the caller's buffer; nothing is copied. Relying on `std::max` over two
> `strrchr` results assumes both point into the same contiguous string so
> that pointer ordering is meaningful.

> [spec:cg3:def:uextras.cg3.find-and-replace-fn]
> size_t findAndReplace(UnicodeString& str, UStringView from, UStringView to)

> [spec:cg3:sem:uextras.cg3.find-and-replace-fn]
> Replaces every non-overlapping occurrence of the substring `from` with
> `to` inside the ICU `UnicodeString` `str`, in place, and returns the
> number of replacements made. `from` and `to` are UTF-16 views
> (`UStringView`). Initialize `rv = 0` and a search offset `offset = 0`.
> Loop: call `str.indexOf(from.data(), from.size(), offset)` (ICU search
> for the `from` code units starting at `offset`); it returns the index of
> the next match or `-1`. While the result is not `-1`: `str.replace(
> offset, from.size(), to.data(), 0, to.size())` overwrites the matched
> span with `to`, then advance `offset += to.size()` (past the just-
> inserted replacement, so replacements are not re-scanned and a `to` that
> contains `from` cannot loop forever), and increment `rv`. Return `rv`.
> Sizes are cast to `int32_t` for the ICU calls.

> [spec:cg3:def:uextras.cg3.get-line-clean-fn]
> size_t get_line_clean(UString& line, UString& cleaned, std::istream& input, bool keep_tabs)

> [spec:cg3:sem:uextras.cg3.get-line-clean-fn]
> Reads one logical line of UTF-16 from `input` into the caller-provided
> buffer `line`, and writes a whitespace-collapsed copy into `cleaned`;
> returns the number of code units written to `cleaned` (its length,
> excluding terminator). Both `line` and `cleaned` are pre-sized `UString`
> buffers that this function may grow. `keep_tabs` controls whether tab
> runs collapse to a tab or to a space. Maintain `offset` (read/scan
> cursor into `line`) and `packoff` (write cursor into `cleaned`), both
> starting at 0.
> Outer loop: call `u_fgets(&line[offset], line.size()-offset-1, input)`
> to read more into `line` at the current cursor; it returns null when it
> read nothing (EOF, or an empty line — see u_fgets quirk). While it
> returns non-null, run an inner `for` loop advancing `offset` while
> `offset < line.size()`:
> - If `line[offset]` is whitespace per `ISSPACE` but not a newline per
>   `ISNL` (note `ISNL` covers 0x0A, 0x0B, 0x0C, 0x2028, 0x2029 but NOT
>   carriage return 0x0D — so `\r` counts as collapsible horizontal
>   space): begin a run. Set `space` to `'\t'` if this char is a tab else
>   `' '`. Consume the whole run (`while ISSPACE && !ISNL` advance
>   `offset`), and if any char in the run is a tab set `space='\t'`. After
>   the run, if `!keep_tabs` force `space=' '`. Write a single `space`
>   into `cleaned[packoff++]`. So each maximal run of horizontal
>   whitespace collapses to exactly one code unit: a tab if `keep_tabs`
>   and the run contained a tab, otherwise a space.
> - Then inspect `line[offset]` (the char that ended the run, or the
>   current char): if it is a newline (`ISNL`), write `cleaned[packoff] =
>   cleaned[packoff+1] = 0` and return `packoff` (line complete). If it is
>   NUL (`== 0`), write the same two terminators and `break` out of the
>   inner loop to read more via the outer loop. Otherwise it is an
>   ordinary char: copy `cleaned[packoff++] = line[offset]`.
> After the inner loop exits (either by `break` on NUL or by `offset`
> reaching `line.size()`), if `packoff > line.size()/2` treat it as
> "buffer too small": double `line` (`resize(line.size()*2, 0)`) and
> resize `cleaned` to `line.size()+1` (the new doubled size, zero-filled),
> then loop to read more. `offset` and `packoff` are NOT reset between
> outer iterations, so reading continues appending into the same buffers.
> If `u_fgets` returns null, exit the outer loop and return `packoff`.
> `cleaned` is left NUL-terminated at index `packoff` on the normal
> newline/NUL exits. The source comment notes the resize heuristic also
> triggers on malformed input where U+0085 (NEL) is mistaken for an
> ellipsis, because NEL is treated as an ordinary character, not
> whitespace, by `ISSPACE`.

> [spec:cg3:def:uextras.cg3.substr-fn]
> inline substr_t<Str> substr(const Str& str, size_t offset = 0, size_t count = 0)

> [spec:cg3:sem:uextras.cg3.substr-fn]
> Convenience factory: constructs and returns a `substr_t<Str>` over `str`
> starting at `offset` with length `count`. Note the default arguments
> differ from `substr_t`'s own constructor: here `offset` defaults to 0
> and `count` defaults to `0` (not `Str::npos`). So `substr(str)` or
> `substr(str, off)` produces a `substr_t` with a defined `count` of 0
> (which the `substr_t` machinery treats as a zero-length,
> NUL-terminate-in-place view whose destructor restores the overwritten
> code unit), rather than the "whole rest of string" (`npos`) that
> `substr_t`'s own default would give. Callers normally pass an explicit
> `count`.

> [spec:cg3:def:uextras.cg3.substr-t]
> struct substr_t {
>   const Str& str;
>   size_t offset, count;
>   value_type old_value;
> }

> [spec:cg3:def:uextras.cg3.substr-t.data-fn]
> const value_type* data() const

> [spec:cg3:sem:uextras.cg3.substr-t.data-fn]
> Returns a NUL-terminated C pointer to the substring [offset, offset+
> count) of the underlying string, by temporarily NUL-terminating in
> place. Compute `buf = const_cast<value_type*>(str.data() + offset)`
> (drops const to write into the string's own storage), write
> `buf[count] = 0` (overwriting the code unit at `str[offset+count]`),
> and return `buf`. The overwritten value was saved into `old_value` by
> the constructor and is restored by `~substr_t`, so the mutation is
> transient for the lifetime of the `substr_t`. Side effect: the backing
> string's buffer is modified while the `substr_t` is alive; the returned
> pointer is only valid (as a `count`-length NUL-terminated string) until
> the `substr_t` is destroyed. Note `count` must not be `Str::npos` for
> this to make sense (writing at `str[offset+npos]` would be out of
> bounds).

> [spec:cg3:def:uextras.cg3.substr-t.substr-t-fn]
> substr_t(const Str& str, size_t offset = 0, size_t count = Str::npos)

> [spec:cg3:sem:uextras.cg3.substr-t.substr-t-fn]
> Constructor for the in-place substring proxy. Stores a reference to
> `str`, and the `offset` and `count` (defaults: `offset=0`,
> `count=Str::npos`). Initializes `old_value = 0`. If `count != Str::npos`
> (i.e., a bounded substring was requested), save the code unit that
> `data()` will later overwrite: `old_value = str[offset + count]`. This
> saved value is what `~substr_t` restores into `str[offset+count]` when
> the proxy is destroyed. When `count == npos`, nothing is saved and the
> destructor performs no restore. No copying of string contents occurs;
> the object only records where the transient NUL terminator will go and
> what to put back.

> [spec:cg3:def:uextras.cg3.substr-t.value-type]
> typedef typename Str::value_type value_type

> [spec:cg3:def:uextras.cg3.ux-bufcpy-fn]
> inline UChar* ux_bufcpy(UChar* dst, const UChar* src, size_t n)

> [spec:cg3:sem:uextras.cg3.ux-bufcpy-fn]
> Copies up to `n` UTF-16 code units from `src` to `dst`, replacing raw
> newline code units with their Unicode "Control Pictures" symbols, and
> NUL-terminates `dst`. Loop `i` from 0 while `i < n` AND `src` is non-null
> AND `src[i] != 0` (stops early at the first NUL in `src`, or immediately
> if `src` is null): set `dst[i] = src[i]`; then if `dst[i]` is LF (0x0A)
> or CR (0x0D), add 0x2400 to it (mapping 0x0A→0x240A "SYMBOL FOR LINE
> FEED", 0x0D→0x240D "SYMBOL FOR CARRIAGE RETURN"). After the loop write
> `dst[i] = 0` (NUL terminator at the number of code units copied) and
> return `dst`. Caller must ensure `dst` has room for at least `i+1` code
> units.

> [spec:cg3:def:uextras.cg3.ux-dirname-fn]
> std::string ux_dirname(const char* in)

> [spec:cg3:sem:uextras.cg3.ux-dirname-fn]
> Returns the directory portion of the path `in` as a `std::string`,
> guaranteed to end in a separator. Uses a 32768-byte zero-initialized
> stack buffer `tmp`. On POSIX: `strcpy(tmp, in)` (no bounds check —
> assumes `in` is shorter than 32768), then `dir = dirname(tmp)` (the
> POSIX `dirname`, which may modify `tmp` in place or return a pointer to
> static storage); if `dir != tmp`, copy the result back with `strcpy(tmp,
> dir)`. On Windows: `GetFullPathNameA(in, 32767, tmp, &fname)` resolves
> the absolute path into `tmp` and sets `fname` to point at the filename
> component within `tmp`; if `fname` is non-null, truncate it by setting
> `fname[0] = 0`, leaving only the directory (with its trailing
> separator). Then compute `tlen = strlen(tmp)`; if the last character
> `tmp[tlen-1]` is neither `'/'` nor `'\\'`, append `'/'`: set
> `tmp[tlen+1] = 0` then `tmp[tlen] = '/'`. Return `tmp` as a
> `std::string`. QUIRK/BUG: if `tmp` is empty (`tlen == 0`), `tmp[tlen-1]`
> reads `tmp[SIZE_MAX]`, an out-of-bounds access (unlikely in practice
> because POSIX `dirname("")` yields `"."`, but latent for empty input).

> [spec:cg3:def:uextras.cg3.ux-is-empty-fn]
> inline bool ux_isEmpty(const UChar* text)

> [spec:cg3:sem:uextras.cg3.ux-is-empty-fn]
> Returns true if the NUL-terminated UTF-16 string `text` is empty or
> consists entirely of whitespace. Compute `length = u_strlen(text)` (code
> units up to the NUL). If `length > 0`, iterate `i` over `[0, length)`:
> if any `text[i]` is not whitespace per `ISSPACE`, return false. If the
> loop completes (or `length == 0`), return true. Whitespace is defined by
> `ISSPACE`: the code units 0x20, 0x09, 0x0A, 0x0D, 0xA0, plus any code
> unit for which ICU `u_isWhitespace` is true (with a fast-path rejection
> of all other code units <= 0xFF).

> [spec:cg3:def:uextras.cg3.ux-is-set-op-fn]
> inline int ux_isSetOp(const UChar* it)

> [spec:cg3:sem:uextras.cg3.ux-is-set-op-fn]
> Classifies whether the NUL-terminated UTF-16 string `it` begins with a
> recognized CG set operator and returns its integer code (from the
> `S_*` enum), or `S_IGNORE` if none matches. It reads up to three code
> units (`it[0]`, `it[1]`, `it[2]`) and switches primarily on `it[1]`:
> - If `it[1] == 0` (a one-code-unit token), switch on `it[0]`:
>   `'|'` → `S_OR`; `'+'` → `S_PLUS`; `'-'` → `S_MINUS`; `'^'` →
>   `S_FAILFAST`; `'\\'` (backslash) → `S_SET_DIFF`; U+2229 (∩ INTERSECTION)
>   → `S_SET_ISECT_U`; U+2206 (∆ INCREMENT, used for symmetric difference)
>   → `S_SET_SYMDIFF_U`; anything else → falls through to `S_IGNORE`.
> - If `it[1]` is `'R'` or `'r'`: then if `it[0]` is `'O'` or `'o'` AND
>   `it[2] == 0`, return `S_OR` (recognizes the two-letter word "OR" in any
>   letter-case combination: "OR"/"Or"/"oR"/"or", but only when it is
>   exactly two code units long). Otherwise → `S_IGNORE`.
> - Any other `it[1]` → `S_IGNORE`.
> Enum values (from Strings.hpp): `S_IGNORE=0`, `S_OR=3`, `S_PLUS=4`,
> `S_MINUS=5`, `S_FAILFAST=8`, `S_SET_DIFF=9`, `S_SET_ISECT_U=10`,
> `S_SET_SYMDIFF_U=11`. Matching parity note: only these exact operators
> are recognized; the alphabetic "OR" match is case-insensitive solely for
> the letters O and R and requires the token to be exactly `O R \0`.

> [spec:cg3:def:uextras.cg3.ux-simplecasecmp-fn]
> inline bool ux_simplecasecmp(const UChar* a, const UChar* b, const size_t n)

> [spec:cg3:sem:uextras.cg3.ux-simplecasecmp-fn]
> Crude ASCII-only, one-directional case-insensitive prefix comparison of
> the first `n` UTF-16 code units of `a` against `b`, with a trailing
> word-boundary check. Loop `i` over `[0, n)`: `a[i]` is considered a
> match for `b[i]` if `a[i] == b[i]` OR `a[i] == b[i] + 32`; on the first
> mismatch return false. The `+32` is the ASCII lowercase offset, so this
> matches only when `a[i]` equals `b[i]` exactly or `a[i]` is the code
> unit 32 above `b[i]`. IMPORTANT ASYMMETRY/QUIRK: it never checks
> `a[i] == b[i] - 32`, so it matches when `a` is the lowercase form and `b`
> is the uppercase form (e.g. `b='A'` 0x41, `a='a'` 0x61), and exact
> equality, but NOT when `a` is uppercase and `b` is lowercase. It also
> blindly adds 32 to any code unit, so it produces false "case" matches
> outside A–Z (e.g. `b='['` 0x5B matches `a='{'` 0x7B, `b='0'` matches
> `a='P'`); no real Unicode case folding is performed. After all `n` code
> units match, return the boundary test: true only if the code unit at
> `a[n]` (immediately past the compared region) is a NUL (`== 0`), OR
> whitespace (`ISSPACE`), OR a delimiter (`ISDELIM`), OR has ICU combining
> class 0 (`u_getCombiningClass(a[n]) == 0`, i.e. `a[n]` is not a combining
> mark). This rejects a match when a combining character immediately
> follows the last plain letter; the disjunction is ordered NUL/space/
> delim first as a short-circuit to avoid the `u_getCombiningClass` call
> for common suffixes. `a` must be readable through index `n` (i.e. at
> least `n+1` code units). Matching parity note: this is the "fast, wrong"
> path — for correct Unicode folding see `ux_strCaseCompare`.

> [spec:cg3:def:uextras.cg3.ux-str-case-compare-fn]
> inline bool ux_strCaseCompare(const UString& a, const UString& b)

> [spec:cg3:sem:uextras.cg3.ux-str-case-compare-fn]
> Proper full-Unicode case-insensitive equality of two UTF-16 strings `a`
> and `b`. Calls ICU `u_strCaseCompare(a.data(), a.size(), b.data(),
> b.size(), U_FOLD_CASE_DEFAULT, &status)`, which case-folds both operands
> using the default Unicode case-folding rules and returns a comparison
> result (<0, 0, or >0). If `status` is anything other than `U_ZERO_ERROR`
> after the call, it throws (see BUG below). Returns `true` iff the
> comparison result is exactly 0 (case-insensitively equal). Unlike
> `ux_simplecasecmp`, this is symmetric and handles non-ASCII case folding
> correctly. BUG/QUIRK: on error it executes `throw new
> std::runtime_error(u_errorName(status))` — it throws a POINTER
> (`std::runtime_error*`), not an exception value; a normal
> `catch(const std::exception&)` will not catch it and the allocation
> leaks. Faithful ports should reproduce that this is an error path that
> does not integrate with value-based exception handling.

> [spec:cg3:def:uextras.read-utf8-fn]
> std::string read_utf8(std::istream& input, size_t BUF_SIZE)

> [spec:cg3:sem:uextras.read-utf8-fn]
> Reads a block of bytes from `input` and returns it as a `std::string`,
> extending the read as needed so the returned bytes end on a complete
> UTF-8 character boundary (assuming the input is valid UTF-8). `BUF_SIZE`
> defaults to 1000 (from the header declaration). Allocate `buf8` of
> `BUF_SIZE` zero bytes. Read `BUF_SIZE - 4` bytes into it (leaving 4
> bytes of headroom, the max length of a UTF-8 sequence); set `sz =
> input.gcount()` (bytes actually read). If the last byte read
> (`buf8[sz-1]`) has its high bit set (`& 0x80`, meaning it is part of a
> multibyte sequence — a lead or continuation byte), scan backwards from
> `i = sz-1`, decrementing `i` each iteration, to find the sequence's lead
> byte and complete it:
> - if `(buf8[i] & 0xF0) == 0xF0` (4-byte lead): let `k = sz-1-i` be the
>   number of continuation bytes already present after the lead; read
>   `3 - k` more bytes into `buf8[sz]`; on read failure throw
>   `std::runtime_error`; then `sz += 3 - k`; break.
> - else if `(buf8[i] & 0xE0) == 0xE0` (3-byte lead): read `2 - k` more;
>   `sz += 2 - k`; break.
> - else if `(buf8[i] & 0xC0) == 0xC0` (2-byte lead): read `1 - k` more;
>   `sz += 1 - k`; break.
> - else (a continuation byte 10xxxxxx: none of the masks match): fall
>   through and continue the loop, moving `i` back toward the lead.
> The mask order (0xF0, then 0xE0, then 0xC0) matters because a 4-byte
> lead also satisfies the 0xE0 and 0xC0 masks; checking the widest first
> classifies correctly. Finally `buf8.resize(sz)` and return it. EDGE/BUG:
> if `sz == 0` (nothing read), `buf8[sz-1]` reads `buf8[SIZE_MAX]`,
> out-of-bounds; and the backward scan has no lower-bound guard, so
> malformed UTF-8 with no lead byte would run `i` below 0 — both are
> latent for empty or invalid input.

> [spec:cg3:def:uextras.u-fflush-fn]
> void u_fflush(std::ostream& output)

> [spec:cg3:sem:uextras.u-fflush-fn]
> Flushes the given output stream: calls `output.flush()`. That is the
> entire body. (A sibling overload taking `std::ostream*` does the same
> via `output->flush()`.) No return value, no error handling beyond
> whatever the stream itself does.

> [spec:cg3:def:uextras.u-fgetc-fn]
> UChar u_fgetc(std::istream& input)

> [spec:cg3:sem:uextras.u-fgetc-fn]
> Reads and returns the next single UTF-16 code unit (`UChar`) from the
> UTF-8 byte stream `input`. Handles non-BMP code points by splitting the
> surrogate pair across two successive calls, using a per-thread cache of
> pending low surrogates keyed by stream pointer.
> - A `static thread_local` array `cps[4]` of `{istream* i, UChar c}` (all
>   zero-initialized) holds up to four pending second-half surrogates.
>   First, scan `cps`: if any entry's `i == &input`, this stream has a
>   pending low surrogate — clear that entry (`i = 0`) and return its
>   stored `c` immediately.
> - Otherwise read one byte via `input.get()` into `c`. If not EOF, store
>   it as `buf[0]` and determine the UTF-8 sequence length from the lead
>   byte: if `(c & 0xF0) == 0xF0` read 3 more bytes; else if
>   `(c & 0xE0) == 0xE0` read 2 more; else if `(c & 0xC0) == 0xC0` read 1
>   more; else (ASCII, high bit clear) read no more. Each short read of the
>   expected continuation bytes throws `std::runtime_error`. `i` becomes
>   the total byte count.
> - If nothing was read and EOF (`i == 0 && c == EOF`), return `U_EOF`
>   (0xFFFF).
> - If `c == 0` (the first byte read was a NUL byte), return `0` directly
>   without conversion.
> - Convert `buf[0..i)` from UTF-8 to UTF-16 via `u_strFromUTF8` into a
>   two-element `u16` array; on ICU failure throw. If `u16[1]` is non-zero
>   the code point was non-BMP and produced a surrogate pair
>   (`u16[0]`=high, `u16[1]`=low): find a free `cps` slot (`i == nullptr`),
>   store `&input` and `u16[1]` so the next call for this stream returns
>   the low surrogate, and return `u16[0]`; if all four slots are occupied,
>   throw `std::runtime_error`. Otherwise return `u16[0]`.
> Notes: returns are 16-bit code units, so callers see surrogate pairs one
> unit at a time; `U_EOF` (0xFFFF) is the end sentinel. The lead-byte masks
> use the same nesting order (0xF0/0xE0/0xC0) as `read_utf8`.

> [spec:cg3:def:uextras.u-fgets-fn]
> UChar* u_fgets(UChar* s, int32_t n, std::istream& input)

> [spec:cg3:sem:uextras.u-fgets-fn]
> Reads UTF-16 code units from `input` into buffer `s` (capacity `n`),
> stopping at end-of-stream or after storing a newline, and returns `s`
> or null. Set `s[0] = 0`. Loop `i` from 0 while `i < n`: read a code unit
> `c = u_fgetc(input)`; if `c == U_EOF` break WITHOUT storing; otherwise
> store `s[i] = c`; if `c` is a newline per `ISNL` break (the newline has
> been stored). After the loop, if `i < n` write `s[i+1] = 0`. If `i == 0`
> return `nullptr`; otherwise return `s`.
> QUIRKS: (1) The terminator is written at `s[i+1]`, not `s[i]`. When the
> loop stopped on a stored newline, `s[i]` is that newline and `s[i+1]=0`
> is a correct terminator. But when it stopped on EOF, no char was stored
> at `s[i]`, so `s[i]` keeps stale/previous content and the NUL lands at
> `s[i+1]` — leaving one uninitialized code unit before the terminator
> (off-by-one on the EOF path). (2) If the VERY FIRST code unit read is a
> newline, the body stores `s[0]=newline` then breaks with `i` still 0,
> so the function writes `s[1]=0` but returns `nullptr` (because `i == 0`).
> Thus an empty line (a line that is just a newline) is reported the same
> as EOF — callers such as `get_line_clean` treat it as "read nothing".
> (3) If the buffer fills exactly (`i` reaches `n` with no break), no
> terminator is written.

> [spec:cg3:def:uextras.u-fprintf-fn]
> inline int32_t _u_fprintf(std::ostream& output, const Char* fmt, va_list args)

> [spec:cg3:sem:uextras.u-fprintf-fn]
> Template core (parametrized on `Char` = `UChar` or `char`) shared by all
> the `u_fprintf` overloads. Formats a printf-style `fmt` with the given
> `va_list args` into UTF-16, converts the result to UTF-8, writes it to
> `output`, and returns the UTF-16 length produced (`n16`). Steps:
> - Stack buffer `_buf16[500]` UChars; `buf16` points at it. `va_copy` the
>   incoming `args` into `args2` (a backup for a possible second pass).
> - `n16 = 500`; call `_u_vsnprintf(buf16, 500, fmt, args)` (dispatched by
>   `Char` type: UChar→`u_vsnprintf_u`, char→`u_vsnprintf`), which returns
>   the number of UChars that WOULD be written. If `n16 < 0` throw
>   `std::runtime_error` ("Critical error in u_fprintf() wrapper").
> - If `n16 > 500` (didn't fit): resize a heap `UString _str16` to `n16+1`,
>   point `buf16` at its data, and re-run `_u_vsnprintf(buf16, n16, fmt,
>   args2)` using the copied args.
> - Stack buffer `_buf8[1500]` bytes (= 500*3); `buf8` points at it;
>   `n8 = 1500`. Call `u_strToUTF8(buf8, n8, &u8, buf16, n16, &err)`
>   converting the `n16` UChars to UTF-8, setting `u8` to the full required
>   byte length. If `u8 > n8` (didn't fit in 1500): resize a heap
>   `std::string _str8` to `u8+1`, point `buf8` at its data, reset `err`,
>   and redo `u_strToUTF8(buf8, u8, 0, buf16, n16, &err)`.
> - `output.write(buf8, u8)` writes exactly `u8` bytes; return `n16`.
> Notes: `err` from the UTF-8 conversions is not inspected (ignored).
> `size(...)` is `std::size` (array element count). Resizing thresholds use
> strict `>`, so a result of exactly 500 UChars or exactly 1500 bytes is
> handled by the stack buffers without a second pass.

> [spec:cg3:def:uextras.u-fprintf-u-fn]
> int32_t u_fprintf_u(std::ostream& output, const UChar* fmt, ...)

> [spec:cg3:sem:uextras.u-fprintf-u-fn]
> Variadic public wrapper taking a UTF-16 (`UChar*`) format string. Sets
> up a `va_list` over the trailing arguments (`va_start`/`va_end`), calls
> the shared `_u_fprintf(output, fmt, args)` core with the UChar format
> (which routes formatting through ICU `u_vsnprintf_u`), and returns its
> result (the number of UTF-16 code units produced). Behaviorally
> identical to the `char*`-format `u_fprintf` overloads except the format
> string and its directives are UTF-16.

> [spec:cg3:def:uextras.u-fputc-fn]
> UChar32 u_fputc(UChar32 c32, std::ostream& output)

> [spec:cg3:sem:uextras.u-fputc-fn]
> Writes a single code point `c32` (a `UChar32`) to `output` encoded as
> UTF-8, and returns `c32` unchanged. Cases:
> - `c32 <= 0x7F`: write the single byte directly via
>   `output.put((char)c32)`.
> - `c32 <= 0x7FFF`: treat `c32` as one BMP `UChar` `c16`; convert that
>   single code unit to UTF-8 via `u_strToUTF8(buf8, 5, &u8, &c16, 1,
>   &err)` and `output.write(buf8, u8)` (1–3 bytes).
> - otherwise (`c32 > 0x7FFF`): throw `std::runtime_error`.
> Return `c32`. BUG/LIMITATION: the second branch's cutoff is 0x7FFF, not
> the full BMP 0xFFFF, so every code point in [0x8000, 0xFFFF] (much of the
> CJK and symbol ranges) and all supplementary code points throw. The
> thrown message reads "can't handle >= 0x7FFF" although 0x7FFF itself IS
> handled (the guard is `> 0x7FFF`). Faithful ports must reproduce that
> `u_fputc` cannot emit code points at or above 0x8000.

> [spec:cg3:def:uextras.u-vsnprintf-fn]
> inline int32_t _u_vsnprintf(UChar* dst, int32_t count, const UChar* fmt, va_list args)

> [spec:cg3:sem:uextras.u-vsnprintf-fn]
> Thin overloaded dispatcher to ICU's formatted-print into a UTF-16
> buffer, selected by the format string's character type. This overload
> takes a `UChar*` format and forwards to `u_vsnprintf_u(dst, count, fmt,
> args)`; the sibling `char*` overload forwards to `u_vsnprintf(dst,
> count, fmt, args)`. Both write up to `count` UChars into `dst` and
> return the number of UChars that WOULD have been written (excluding the
> terminator), per ICU `u_vsnprintf`/`u_vsnprintf_u` semantics; that
> return value is what the `_u_fprintf` core uses to detect truncation.

> [spec:cg3:def:uextras.ux-strip-bom-fn]
> inline bool ux_stripBOM(std::istream& stream)

> [spec:cg3:sem:uextras.ux-strip-bom-fn]
> Detects and consumes a UTF-8 byte-order mark (bytes 0xEF 0xBB 0xBF) at
> the current position of `stream`; returns true if a BOM was present (and
> consumed) or false otherwise (with the stream left exactly as it was
> found). Read byte `a` via `stream.get()`. If it is EOF, return false
> (nothing consumed). If `a != 0xEF`, `putback` `a` and return false.
> Otherwise read byte `b`: if EOF, put back `a` and return false; if
> `b != 0xBB`, put back `b` then `a` (reverse order to restore the stream)
> and return false. Otherwise read byte `c`: if EOF, put back `b` then `a`
> and return false; if `c != 0xBF`, put back `c`, `b`, `a` and return
> false. If all three matched, return true, leaving the three BOM bytes
> consumed (not put back). Comparisons use the unsigned byte value from
> `get()` against 0xEF/0xBB/0xBF; put-backs cast to `char`. Relies on the
> stream supporting up to three successive `putback` calls.

