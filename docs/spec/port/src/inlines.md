# src/inlines.hpp

> [spec:cg3:def:inlines.cg3.backtonl-fn]
> inline void BACKTONL(Char*& p)

> [spec:cg3:sem:inlines.cg3.backtonl-fn]
> Walks the reference pointer `p` BACKWARDS. Loop condition: while the
> current char `*p` is non-zero AND `*p` is not a newline (per ISNL, which
> covers U+2028, U+2029, FF, VT, LF but NOT CR) AND `*p` is not an
> UNESCAPED semicolon (i.e. `*p != ';'` OR `ISESC(p)` reports it escaped),
> decrement `p` by one. When the loop stops (p sits on a NUL, a newline,
> or an unescaped ';'), do a single `++p` so p ends pointing one past that
> stop character (i.e. at the first character of the line/segment). No
> lower-bound check: relies on the buffer containing a stop char before the
> start. Mutates p in place; reads chars before p via ISESC.

> [spec:cg3:def:inlines.cg3.begin-fn]
> auto begin(Reversed<T> c)

> [spec:cg3:sem:inlines.cg3.begin-fn]
> Free function found by ADL for a `Reversed<T>` wrapper. Returns
> `std::rbegin(c.t)` — the reverse-iterator to the last element of the
> wrapped container reference `c.t`. Paired with `end(Reversed<T>)` it makes
> a range-based for over a `Reversed<T>` iterate in reverse order.

> [spec:cg3:def:inlines.cg3.cg3-quit-fn]
> [[noreturn]]

> [spec:cg3:sem:inlines.cg3.cg3-quit-fn]
> `CG3Quit(int32_t c = 0, const char* file = nullptr, uint32_t line = 0)`,
> marked `[[noreturn]]`. If BOTH `file` is non-null AND `line` is non-zero,
> it flushes `std::cerr` (`std::cerr << std::flush`) then writes the line
> `CG3Quit triggered from <file> line <line>.` followed by `std::endl`
> (newline + flush) to `std::cerr`. Then it calls `exit(c)`, terminating the
> process with exit code `c` and running atexit/static destructors. Never
> returns. If file is null or line is 0, no message is printed — it just
> exits.

> [spec:cg3:def:inlines.cg3.clear-fn]
> inline void clear(C& c)

> [spec:cg3:sem:inlines.cg3.clear-fn]
> Container-clearing helper guarding a legacy compiler quirk. If `c.empty()`
> is false, calls `c.clear()`; if already empty, does nothing. Net result:
> `c` is empty afterwards. Only side effect is emptying the container.

> [spec:cg3:def:inlines.cg3.concat-fn]
> inline std::string concat(const T& value, Args... args)

> [spec:cg3:sem:inlines.cg3.concat-fn]
> Variadic string builder. Constructs a `std::string msg` from the first
> argument `value` (i.e. `std::string msg(value)` — value must be a type a
> std::string can be constructed from, e.g. `const char*` or std::string),
> then calls `details::_concat(msg, args...)` to append every remaining
> argument in order, then returns `msg` by value. Effect: concatenates all
> arguments into one std::string. With a single argument it just returns a
> std::string copy of it.

> [spec:cg3:def:inlines.cg3.details.concat-fn]
> inline void _concat(std::string& msg, const T& t, Args... args)

> [spec:cg3:sem:inlines.cg3.details.concat-fn]
> Recursive tail of the variadic concat. Appends `t` to `msg`
> (`msg.append(t)`; t must be appendable to std::string, e.g. char* or
> std::string), then recurses with `_concat(msg, args...)` on the remaining
> arguments. Recursion terminates at the non-template overload
> `_concat(std::string&)` (zero extra args) which does nothing. Mutates
> `msg` in place; returns void.

> [spec:cg3:def:inlines.cg3.end-fn]
> auto end(Reversed<T> c)

> [spec:cg3:sem:inlines.cg3.end-fn]
> ADL companion to `begin(Reversed<T>)`. Returns `std::rend(c.t)` — the
> reverse-iterator one-before-the-first element of the wrapped container
> `c.t`. Together with `begin` this yields reverse iteration in a range-for.

> [spec:cg3:def:inlines.cg3.erase-fn]
> inline void erase(Cont& cont, const T& val)

> [spec:cg3:sem:inlines.cg3.erase-fn]
> Erase-remove idiom. Calls
> `cont.erase(std::remove(cont.begin(), cont.end(), val), cont.end())`:
> `std::remove` shifts every element NOT equal to `val` toward the front
> (comparing with `==`), preserving their relative order, and returns the
> iterator to the new logical end; `cont.erase` then drops the tail. Effect:
> removes ALL elements equal to `val` from `cont`. Mutates cont in place;
> returns void.

> [spec:cg3:def:inlines.cg3.g-app-set-opts-ranged-fn]
> inline void GAppSetOpts_ranged(const char* value, Cont& cont, bool fill = true)

> [spec:cg3:sem:inlines.cg3.g-app-set-opts-ranged-fn]
> Parses a comma-separated list of numbers and inclusive numeric ranges
> from the C string `value` into `cont` (a container supporting `clear()`,
> `push_back(uint32_t)`, `size()`, `front()`). Steps: clear `cont`; set
> `had_range = false`; set cursor `comma = value`. Then a do-while loop:
> (1) `low = abs(atoi(comma))` parses the leading integer at `comma` and
> takes its absolute value; `high = low`. (2) `delim = strchr(comma, '-')`
> finds the first '-'; `nextc = strchr(comma, ',')` finds the first ','.
> (3) If `delim` is non-null AND (`nextc` is null OR `nextc > delim`, i.e.
> the dash belongs to THIS token, before the next comma), set
> `had_range = true` and `high = abs(atoi(delim + 1))` (parse the number
> after the dash). (4) For `low` up to and INCLUDING `high`, `push_back`
> each value; because both are uint32_t, if `high < low` (e.g. "3-1") no
> values are pushed. The do-while continues while advancing
> `comma = strchr(comma, ',')` yields a non-null pointer, then `++comma`
> steps past that comma, and `*comma != 0` (more text remains). After the
> loop: if `cont.size() == 1` AND `!had_range` AND `fill` is true, take the
> single value `val = cont.front()`, clear `cont`, and push `1,2,...,val`.
> So a lone bare number N with fill=true expands to the full range 1..N;
> any explicit range or multiple entries suppresses that expansion. Note
> `atoi` reads only the leading integer and ignores trailing junk, `abs`
> discards the sign, and `nextc` is used only inside the range condition.

> [spec:cg3:def:inlines.cg3.hash-ustring]
> struct hash_ustring

> [spec:cg3:def:inlines.cg3.hash-ustring.operator-fn]
> size_t operator()(const UString& str) const

> [spec:cg3:sem:inlines.cg3.hash-ustring.operator-fn]
> Hash functor used as the Hash type for unordered containers keyed on
> UString. `operator()(const UString& str) const` returns
> `hash_value(str)`, i.e. `hash_value(str.data(), 0, str.size())` which
> forces the seed to CG3_HASH_SEED and runs SuperFastHash over the UTF-16
> code units of the string (see hash-value / super-fast-hash). Return type
> is `size_t` (the uint32_t hash zero-extended). A sibling
> `operator()(const UStringView&)` (not separately specced) does the same
> over a string view.

> [spec:cg3:def:inlines.cg3.hash-value-fn]
> inline uint32_t hash_value(uint32_t c, uint32_t h = CG3_HASH_SEED)

> [spec:cg3:sem:inlines.cg3.hash-value-fn]
> Mixes a single 32-bit value `c` into a running 32-bit hash `h` (default
> seed `CG3_HASH_SEED = 705577479`). If `h == 0`, reset `h = CG3_HASH_SEED`
> first. Then compute, in 32-bit unsigned wraparound arithmetic,
> `h = c + (h << 6) + (h << 16) - h`. That algebraically equals
> `c + h*(64 + 65536 - 1) = c + h*65599 (mod 2^32)` — the classic *65599
> mixing plus the value c. Then a reserved-value remap: if the result `h`
> is 0, `0xFFFFFFFF` (uint32 max), or `0xFFFFFFFE` (max-1), set
> `h = CG3_HASH_SEED`. Return `h`. IMPORTANT: this is a self-contained
> integer mixer; the SuperFastHash-based alternative in the source is
> commented out and NOT used. The port must reproduce this exact 65599-style
> formula and the three-value remap for stable hashes.

> [spec:cg3:def:inlines.cg3.inc-dec]
> class inc_dec {
>   T* p;
> }

> [spec:cg3:def:inlines.cg3.inc-dec.inc-dec-fn]
> ~inc_dec()

> [spec:cg3:sem:inlines.cg3.inc-dec.inc-dec-fn]
> Destructor of the RAII counter guard. If the stored member pointer `p` is
> non-null, decrement the pointed-to value (`--(*p)`). The default
> constructor initializes `p = nullptr`, so if `inc()` was never called the
> destructor does nothing. This undoes the increment previously applied by
> `inc()`.

> [spec:cg3:def:inlines.cg3.inc-dec.inc-fn]
> void inc(T& pt)

> [spec:cg3:sem:inlines.cg3.inc-dec.inc-fn]
> Stores the address of the referenced value `pt` into member `p`
> (`p = &pt`), then increments that value (`++(*p)`). Effect: increments
> `pt` immediately and records it so the destructor will later decrement it.
> If called more than once, only the most recently `inc`'d value is
> remembered/decremented.

> [spec:cg3:def:inlines.cg3.insert-if-exists-fn]
> inline void insert_if_exists(boost::dynamic_bitset<>& cont, const boost::dynamic_bitset<>* other)

> [spec:cg3:sem:inlines.cg3.insert-if-exists-fn]
> Unions the bits of `*other` into `cont`. If `other` is non-null AND
> `!other->empty()`: first grow `cont` via
> `cont.resize(std::max(cont.size(), other->size()))` (new high bits are
> zero-filled), then perform `cont |= *other` (bitwise OR-assign). After the
> resize `cont.size() >= other->size()`, satisfying dynamic_bitset's
> requirement that operands of `|=` be equal-sized. If `other` is null or
> empty, do nothing. Mutates `cont`; returns void.

> [spec:cg3:def:inlines.cg3.is-cg3b-fn]
> inline bool is_cg3b(const S& s)

> [spec:cg3:sem:inlines.cg3.is-cg3b-fn]
> Returns true iff the first four elements of `s` are exactly `'C'`, `'G'`,
> `'3'`, `'B'` (`s[0]=='C' && s[1]=='G' && s[2]=='3' && s[3]=='B'`) — the
> magic prefix of a compiled `.cg3b` binary grammar. Indexes `s[0..3]`
> without any bounds check; caller must guarantee length >= 4.

> [spec:cg3:def:inlines.cg3.is-cg3bsf-fn]
> inline bool is_cg3bsf(const S& s)

> [spec:cg3:sem:inlines.cg3.is-cg3bsf-fn]
> Returns true iff the first four elements of `s` are exactly `'C'`, `'G'`,
> `'B'`, `'F'` (`s[0]=='C' && s[1]=='G' && s[2]=='B' && s[3]=='F'`) — the
> "CGBF" magic distinct from the "CG3B" checked by `is_cg3b`. Indexes
> `s[0..3]` without bounds checking; length >= 4 required.

> [spec:cg3:def:inlines.cg3.is-icase-fn]
> inline size_t IS_ICASE(const Char* p, const C (&uc)[N], const C (&lc)[N])

> [spec:cg3:sem:inlines.cg3.is-icase-fn]
> Case-insensitive fixed keyword matcher at buffer position `p`. `uc` and
> `lc` are two equal-length arrays of size `N` holding the uppercase and
> lowercase forms of a keyword; because they are string literals with a NUL
> terminator, the keyword length is `N - 1`. Steps: (1) If
> `ISSTRING(p, N - 1)` is true (the surrounding region is a quoted `"..."`
> or `<...>` literal), return 0 — do not treat text inside a string as a
> keyword. (2) Loop `i` from 0 to `N - 2` inclusive (N-1 characters): for
> each, if `p[i]` equals neither `uc[i]` nor `lc[i]`, return 0. (3) After
> matching all N-1 characters, inspect the following char `p[N - 1]`: if it
> is NOT alphanumeric (`u_isalnum(p[N-1])` is false — a word boundary),
> return `i` (which equals N-1, the number of characters matched);
> otherwise (an alphanumeric continuation, so this is a longer word) return
> 0. Return value is the matched length (N-1) on success, 0 on failure.
> Note it reads `p[-1]` and `p[N-1]` via ISSTRING and the boundary check.

> [spec:cg3:def:inlines.cg3.is-internal-fn]
> inline bool is_internal(const S& s)

> [spec:cg3:sem:inlines.cg3.is-internal-fn]
> Returns true iff `s` begins with the three-element sequence `'_'`, `'G'`,
> `'_'` (`s[0]=='_' && s[1]=='G' && s[2]=='_'`), the marker prefix for
> CG3-internal names. Indexes `s[0..2]` with no bounds check; caller must
> ensure length >= 3.

> [spec:cg3:def:inlines.cg3.is-textual-fn]
> inline bool is_textual(const S& s)

> [spec:cg3:sem:inlines.cg3.is-textual-fn]
> Returns true iff `s` is delimited as a textual/tag literal: either
> `s.front() == '"' && s.back() == '"'` (double-quoted) OR
> `s.front() == '<' && s.back() == '>'` (angle-bracketed). Uses `front()`
> and `back()`, so behavior is undefined on an empty `s`. Note a single
> `'"'` character has front == back == '"' and would return true; a single
> `'<'` returns false (front '<' but back '<' != '>').

> [spec:cg3:def:inlines.cg3.isalpha-c-fn]
> inline bool ISALPHA_C(Char p)

> [spec:cg3:sem:inlines.cg3.isalpha-c-fn]
> Returns `(p < 255) && isalpha(p)` — true iff `p` is strictly less than 255
> AND the C library `isalpha(p)` (locale-dependent, normally the "C" locale)
> is non-zero. The `p < 255` guard (note: strict `<`, so code point 255 is
> excluded) limits evaluation to the low range. FAITHFULNESS/UB NOTE: `p` is
> passed straight to `isalpha` without casting to `unsigned char`; if `Char`
> is a signed type and `p` is negative, `p < 255` is still true and
> `isalpha` with a negative argument other than EOF is technically UB — the
> port should replicate the literal `(p < 255) && isalpha(p)` guarding.

> [spec:cg3:def:inlines.cg3.isdelim-fn]
> inline bool ISDELIM(const UChar c)

> [spec:cg3:sem:inlines.cg3.isdelim-fn]
> Returns true iff `c` is one of the nine delimiter characters
> `(` `)` `+` `-` `*` `/` `^` `%` `=`, evaluated as
> `c=='(' || c==')' || c=='+' || c=='-' || c=='*' || c=='/' || c=='^' ||
> c=='%' || c=='='`. Otherwise false.

> [spec:cg3:def:inlines.cg3.isdigit-c-fn]
> inline bool ISDIGIT_C(Char p)

> [spec:cg3:sem:inlines.cg3.isdigit-c-fn]
> Returns `(p < 255) && isdigit(p)` — true iff `p < 255` (strict) AND the C
> library `isdigit(p)` is non-zero. Same low-range guard and same
> signed-char UB caveat as ISALPHA_C: `p` is not cast to unsigned char
> before being passed to `isdigit`.

> [spec:cg3:def:inlines.cg3.isesc-fn]
> inline bool ISESC(const Char* p)

> [spec:cg3:sem:inlines.cg3.isesc-fn]
> Determines whether the character at `p` is backslash-escaped by counting
> consecutive backslashes immediately BEFORE it. Initialize `a = 1`; while
> `*(p - a) == '\\'`, increment `a`. So if there are `k` consecutive
> backslashes just before `p` (`p[-1], p[-2], ...`), the loop ends with
> `a = k + 1`. Returns `(a % 2 == 0)`, i.e. true when `a` is even, which
> happens exactly when `k` (the backslash count) is ODD. Meaning: returns
> true (the char is escaped) iff an odd number of backslashes precede `p`.
> Reads memory before `p` with no lower bound — relies on a non-backslash
> sentinel existing before the buffer start.

> [spec:cg3:def:inlines.cg3.isnl-fn]
> inline bool ISNL(const UChar c)

> [spec:cg3:sem:inlines.cg3.isnl-fn]
> Newline predicate. Returns true iff `c` is one of: U+2028 (Line
> Separator), U+2029 (Paragraph Separator), U+000C (Form Feed), U+000B
> (Vertical Tab), or U+000A (ASCII LF `\n`). Otherwise false. IMPORTANT:
> U+000D (CR `\r`) is deliberately NOT included, so `\r` is not treated as a
> line break by this predicate.

> [spec:cg3:def:inlines.cg3.isspace-fn]
> inline bool ISSPACE(const UChar c)

> [spec:cg3:sem:inlines.cg3.isspace-fn]
> Whitespace predicate for a 16-bit UChar. Fast path: if `c <= 0xFF` AND `c`
> is not one of the five Latin-1 whitespace code points 0x09 (TAB), 0x0A
> (LF), 0x0D (CR), 0x20 (SPACE), 0xA0 (NBSP), return false immediately.
> Otherwise (i.e. `c > 0xFF`, or `c` is one of those five) return
> `c==0x20 || c==0x09 || c==0x0A || c==0x0D || c==0xA0 || u_isWhitespace(c)`.
> Effect: recognizes ASCII/Latin-1 tab, LF, CR, space and NBSP, plus any
> character ICU's `u_isWhitespace` classifies as whitespace (which applies
> only for `c > 0xFF` given the fast reject). QUIRK: NBSP (0xA0) is treated
> as whitespace here via the explicit test even though ICU `u_isWhitespace`
> does not consider NBSP whitespace.

> [spec:cg3:def:inlines.cg3.isstring-fn]
> inline bool ISSTRING(const Char* p, const uint32_t c)

> [spec:cg3:sem:inlines.cg3.isstring-fn]
> Tests whether a run at `p` is wrapped in matching string delimiters, given
> a length-like argument `c`. Checks the char just before `p` (`p[-1]`) and
> the char at `p[c + 1]`. Returns true if `p[-1]=='"' && p[c+1]=='"'`, OR if
> `p[-1]=='<' && p[c+1]=='>'`; otherwise false. Reads out-of-nominal-bounds
> memory at `p[-1]` and `p[c+1]` (caller must ensure validity). QUIRK/POSSIBLE
> OFF-BY-ONE: it inspects `p[c+1]`, not `p[c]`. Callers pass `c` = the token
> length (e.g. IS_ICASE calls `ISSTRING(p, N-1)` where N-1 is the keyword
> length, so the token occupies p[0..c-1]); the closing delimiter is checked
> at index `c+1` (two past the last token char), not at `c`. The port must
> reproduce the exact `p[c+1]` index.

> [spec:cg3:def:inlines.cg3.make-64-fn]
> inline constexpr uint64_t make_64(uint32_t hi, uint32_t low)

> [spec:cg3:sem:inlines.cg3.make-64-fn]
> constexpr. Combines two 32-bit values into one 64-bit value:
> `(UI64(hi) << 32) | UI64(low)` — zero-extend `hi` to uint64, shift it into
> the high 32 bits, and OR in the zero-extended `low`. Result: `hi` in bits
> 63..32, `low` in bits 31..0.

> [spec:cg3:def:inlines.cg3.make-array-fn]
> constexpr auto make_array(Function f) -> std::array<typename std::invoke_result<Function, std::size_t>::type, N>

> [spec:cg3:sem:inlines.cg3.make-array-fn]
> Template `<int N, class Function>`, constexpr. Builds a `std::array` of
> length `N` whose element `i` equals `f(i)`. It delegates to
> `make_array_helper(f, std::make_index_sequence<N>{})`, passing the
> compile-time index sequence 0..N-1. The element type is
> `std::invoke_result<Function, std::size_t>::type` — the return type of
> calling `f` with a `std::size_t`. Pure/compile-time when `f` is constexpr.

> [spec:cg3:def:inlines.cg3.make-array-helper-fn]
> constexpr auto make_array_helper(Function f, std::index_sequence<Indices...>) -> std::array<typename std::invoke_result<Function, std::size_t>::type, sizeof....

> [spec:cg3:sem:inlines.cg3.make-array-helper-fn]
> Template `<class Function, std::size_t... Indices>`, constexpr. Given the
> callable `f` and a compile-time pack of indices, returns a `std::array`
> brace-initialized as `{ { f(Indices)... } }` — i.e. it calls `f` once per
> index in the pack, in order (`f(I0), f(I1), ...`), and aggregates the
> results. The array's size equals `sizeof...(Indices)` and its element type
> is the return type of `f(std::size_t)`.

> [spec:cg3:def:inlines.cg3.read-be-fn]
> inline double readBE(std::istream& stream)

> [spec:cg3:sem:inlines.cg3.read-be-fn]
> Full template specialization of `readBE<double>` — inverse of
> `writeBE(ostream, double)`. Reads the custom mantissa/exponent big-endian
> encoding: (1) `mant64 = readBE<uint64_t>(stream)` reads 8 bytes big-endian
> into a uint64; (2) `exp = (int)readBE<int32_t>(stream)` reads the next 4
> bytes big-endian into an int32 and widens to int. Then computes
> `value = double(int64_t(mant64)) / double(INT64_MAX)` (reinterpret the
> mantissa bits as a signed int64, convert to double, divide by
> `std::numeric_limits<int64_t>::max()`), and returns `ldexp(value, exp)` =
> `value * 2^exp`. Reads exactly 12 bytes total, mantissa first then
> exponent, big-endian. (The generic `readBE<T>` template it specializes
> reads `sizeof(T)` raw bytes then applies `be::big_to_native`.)

> [spec:cg3:def:inlines.cg3.read-le-fn]
> inline T readLE(std::istream& stream)

> [spec:cg3:sem:inlines.cg3.read-le-fn]
> Return-by-value little-endian reader, template `<T>`. Declares a `T value`,
> reads exactly `sizeof(T)` raw bytes from the stream into its object
> representation via `readRaw` (`stream.read((char*)&value, sizeof(T))`),
> then returns `be::little_to_native(value)` — interpreting the bytes as
> little-endian and converting to host order. On a little-endian host this
> conversion is a no-op; on a big-endian host it byte-swaps. (Distinct from
> the two-argument `readLE(S&, T&)` overload, which reads in place via
> `be::little_to_native_inplace`.)

> [spec:cg3:def:inlines.cg3.read-raw-fn]
> inline void readRaw(S& stream, T& value)

> [spec:cg3:sem:inlines.cg3.read-raw-fn]
> Reads `sizeof(T)` bytes verbatim from `stream` directly into the object
> representation of `value`: `stream.read(reinterpret_cast<char*>(&value),
> sizeof(T))`. No endian conversion and no formatting — the raw in-memory
> bytes (host byte order) are filled. Whatever `std::istream::read` does on
> short reads (sets failbit/eofbit, leaves value partially written) applies.
> Returns void.

> [spec:cg3:def:inlines.cg3.read-utf8-le-fn]
> inline void readUTF8_LE(S& input, Str& rv)

> [spec:cg3:sem:inlines.cg3.read-utf8-le-fn]
> Reads a length-prefixed UTF-8 string and decodes it to UTF-16 into `rv`
> (a UString-like output). Steps: (1) Read a `uint16_t len` LITTLE-ENDIAN
> via `readLE(input, len)` (readRaw + `little_to_native_inplace`) — this is
> the count of UTF-8 BYTES that follow. (2) `rv.clear()` then
> `rv.resize(len)` (pre-size the UTF-16 buffer to `len` code units as an
> upper bound). (3) Read exactly `len` bytes from `input` into a
> `std::vector<char> buffer(len)`. (4) `u_strFromUTF8(&rv[0], len, &olen,
> &buffer[0], len, &status)` decodes UTF-8 -> UTF-16 (destination capacity
> `len`, source length `len`), storing the produced code-unit count in
> `olen`. (5) `rv.resize(olen)` shrinks `rv` to the actual decoded length.
> The ICU `status` is ignored. Writes into `rv` by reference; returns void.

> [spec:cg3:def:inlines.cg3.read-utf8-raw-fn]
> inline UString readUTF8_Raw(S& input)

> [spec:cg3:sem:inlines.cg3.read-utf8-raw-fn]
> Same shape as `readUTF8_LE` but reads the length prefix RAW (host byte
> order) and returns the result. Steps: (1) `uint16_t len = 0;
> readRaw(input, len)` — reads the 2-byte length prefix with NO endian
> conversion (host-endian). (2) Construct `UString rv(len, 0)` — `len` code
> units of value 0. (3) Read `len` bytes into `std::vector<char>
> buffer(len)`. (4) `u_strFromUTF8(&rv[0], len, &olen, &buffer[0], len,
> &status)` decodes UTF-8 -> UTF-16, `olen` = produced code units. (5)
> `rv.resize(olen)`. Returns `rv` by value. BYTE-PARITY NOTE: unlike
> `readUTF8_LE`, the length prefix here is read in native byte order via
> readRaw (so it is host-endian dependent, whereas the _LE variant forces
> little-endian).

> [spec:cg3:def:inlines.cg3.reverse-fn]
> inline T* reverse(T* head)

> [spec:cg3:sem:inlines.cg3.reverse-fn]
> In-place reversal of a singly linked list linked through a public `->next`
> pointer. Standard three-pointer reversal: initialize `nr = nullptr`
> (new head). While `head` is non-null: save `next = head->next`; set
> `head->next = nr`; advance `nr = head`; advance `head = next`. Return `nr`
> — the new head (the former tail). If `head` is null on entry, returns null.
> Mutates every node's `->next` link.

> [spec:cg3:def:inlines.cg3.reversed]
> struct Reversed {
>   T& t;
> }

> [spec:cg3:def:inlines.cg3.reversed-fn]
> Reversed<T> reversed(T&& c)

> [spec:cg3:sem:inlines.cg3.reversed-fn]
> Wraps `c` into a `Reversed<T>` aggregate (`return { c };`) whose member
> `T& t` is a reference bound to `c`. Combined with the ADL `begin`/`end`
> overloads for `Reversed<T>` (which return `rbegin`/`rend`), this lets
> `for (auto& x : reversed(container))` iterate `container` in reverse. Note
> the wrapper stores a REFERENCE, so `c` must outlive the use of the result
> (as it does in a range-for that keeps the temporary alive). `T` is deduced
> from the forwarding reference.

> [spec:cg3:def:inlines.cg3.scope-guard]
> class scope_guard {
>   std::function<void()> func;
>   bool good = true;
> }

> [spec:cg3:def:inlines.cg3.scope-guard.scope-guard-fn]
> ~scope_guard()

> [spec:cg3:sem:inlines.cg3.scope-guard.scope-guard-fn]
> Destructor of the RAII cleanup guard. If member `good` is true, it invokes
> the stored callable `func()`. If `good` is false (the guard was disarmed
> via `set(false)`), it does nothing. So the callable supplied at
> construction runs at scope exit unless disarmed. If `func` is empty and
> `good` is true, invoking it throws `std::bad_function_call`.

> [spec:cg3:def:inlines.cg3.scope-guard.set-fn]
> void set(bool val = true)

> [spec:cg3:sem:inlines.cg3.scope-guard.set-fn]
> Sets the guard's armed flag: `good = val` (default `true`). `set(false)`
> disarms the guard so its destructor will NOT call `func`; `set(true)`
> re-arms it. Returns void.

> [spec:cg3:def:inlines.cg3.size-fn]
> inline constexpr size_t size(T (&)[N])

> [spec:cg3:sem:inlines.cg3.size-fn]
> constexpr. Returns the compile-time element count `N` of a C array passed
> by reference (`T (&)[N]`). The parameter is unnamed — only its type is
> used. No runtime computation; returns `N`.

> [spec:cg3:def:inlines.cg3.skipln-fn]
> inline uint32_t SKIPLN(Char*& p)

> [spec:cg3:sem:inlines.cg3.skipln-fn]
> Advances the reference pointer `p` to the end of the current line, then
> past the line break. Loop: while `*p` is non-zero AND `!ISNL(*p)`,
> increment `p`. After the loop (p on a NUL or a newline char), do one more
> `++p`, stepping past the newline. Always returns `1` (counting one line
> skipped). Mutates `p`. NOTE: ISNL does not include CR (0x0D), so a lone
> `\r` does not stop it. If the string terminates (NUL) before any newline,
> `p` is still advanced one past the NUL — caller's responsibility.

> [spec:cg3:def:inlines.cg3.skipto-fn]
> inline uint32_t SKIPTO(Char*& p, const UChar a)

> [spec:cg3:sem:inlines.cg3.skipto-fn]
> Advances `p` forward until it points at an UNESCAPED occurrence of char
> `a`, or a NUL. Loop while `*p` is non-zero AND (`*p != a` OR `ISESC(p)`
> reports the `a` escaped): each iteration, if `ISNL(*p)` (a newline),
> increment counter `s`; then `++p`. Returns `s` = the number of newlines
> passed over. On exit `p` points at the matching unescaped `a` (or the
> terminating NUL). Mutates `p`.

> [spec:cg3:def:inlines.cg3.skipto-nospan-fn]
> inline void SKIPTO_NOSPAN(Char*& p, const UChar a)

> [spec:cg3:sem:inlines.cg3.skipto-nospan-fn]
> Like SKIPTO but does NOT span newlines and does not count. Loop while `*p`
> non-zero AND (`*p != a` OR `ISESC(p)`): if `ISNL(*p)` (newline),
> `break` immediately (stopping ON the newline); otherwise `++p`. On exit
> `p` sits at the unescaped `a`, a newline, or the NUL. Returns void;
> mutates `p`.

> [spec:cg3:def:inlines.cg3.skipto-nospan-raw-fn]
> inline void SKIPTO_NOSPAN_RAW(Char*& p, const UChar a)

> [spec:cg3:sem:inlines.cg3.skipto-nospan-raw-fn]
> Like SKIPTO_NOSPAN but "raw" — it does NOT honor escaping. Loop while `*p`
> non-zero AND `*p != a` (any occurrence of `a`, escaped or not, stops it):
> if `ISNL(*p)` (newline), `break`; otherwise `++p`. On exit `p` is at `a`,
> a newline, or the NUL. Returns void; mutates `p`.

> [spec:cg3:def:inlines.cg3.skiptows-fn]
> inline uint32_t SKIPTOWS(Char*& p, const UChar a = 0, const bool allowhash = false, const bool allowscol = false)

> [spec:cg3:sem:inlines.cg3.skiptows-fn]
> Advances `p` forward to the next whitespace or to a stop character,
> counting newlines into `s`. Params: `a` = an extra stop char (default 0),
> `allowhash` (default false), `allowscol` (default false). `s = 0`. Loop
> while `*p` is non-zero AND `!ISSPACE(p)` (the POINTER form of ISSPACE,
> i.e. `ISSPACE(*p) && !ISESC(p)` — stops only at UNESCAPED whitespace).
> Inside the loop, in this exact order: (1) if `!allowhash` AND `*p` is an
> unescaped `'#'`, do `s += SKIPLN(p)` (skips the rest of the comment line,
> adding 1, and leaves `p` one past the newline) then `--p`. (2) if
> `ISNL(*p)`, do `++s; ++p`. (3) if `!allowscol` AND `*p` is an unescaped
> `';'`, `break`. (4) if `*p == a` AND `p` is unescaped, `break`. (5) `++p`.
> Returns `s` (newline count). On exit `p` is at unescaped whitespace, a
> stop char (`;` or `a`), or NUL. FAITHFULNESS QUIRKS: (a) after the `'#'`
> branch (`SKIPLN` then `--p` puts `p` on the terminating newline) the very
> next statement `if (ISNL(*p))` fires on that newline and does `++s` again,
> so a comment line increments `s` twice (once from SKIPLN's return, once
> from the ISNL block) for its single newline. (b) When a bare newline is
> processed, step (2) does `++p` and then step (5) does another `++p`, so
> the character immediately after a newline is stepped over after only
> being checked against `;` and `a` (not against whitespace). Reproduce the
> statement order exactly.

> [spec:cg3:def:inlines.cg3.skipws-fn]
> inline uint32_t SKIPWS(Char*& p, const UChar a = 0, const UChar b = 0, const bool allowhash = false)

> [spec:cg3:sem:inlines.cg3.skipws-fn]
> Skips whitespace forward, counting newlines into `s`. Params: `a`, `b` =
> two stop chars (default 0), `allowhash` (default false). `s = 0`. Loop
> while `*p` non-zero AND `*p != a` AND `*p != b`. Inside, in order:
> (1) if `ISNL(*p)`, `++s`. (2) if `!allowhash` AND `*p` is an unescaped
> `'#'`, `s += SKIPLN(p)` (skip comment line, +1, leaving `p` one past the
> newline) then `p--` (back onto the newline). (3) if `!ISSPACE(*p)` (the
> VALUE form of ISSPACE — note: NO escape check here, unlike SKIPTOWS which
> uses the escape-aware pointer form), `break`. (4) `++p`. Returns `s`
> (newline count). On exit `p` is at the first non-whitespace char, a stop
> char `a`/`b`, or NUL. QUIRK: after the `'#'` branch backs `p` onto the
> newline, the next loop iteration's step (1) counts that newline again, so
> a comment line contributes twice to `s` (SKIPLN's +1 plus a later ISNL
> +1), matching SKIPTOWS. Key difference from SKIPTOWS: the stop test uses
> `!ISSPACE(*p)` (value form, escape-INsensitive).

> [spec:cg3:def:inlines.cg3.super-fast-hash-fn]
> inline uint32_t SuperFastHash(const char* data, size_t len = 0, uint32_t hash = CG3_HASH_SEED)

> [spec:cg3:sem:inlines.cg3.super-fast-hash-fn]
> Paul Hsieh's SuperFastHash over a byte buffer. Signature
> `(const char* data, size_t len = 0, uint32_t hash = CG3_HASH_SEED)`, all
> arithmetic in 32-bit unsigned wraparound. Steps: (1) If the seed
> `hash == 0`, set `hash = UI32(len)` (low 32 bits of the length). (Default
> seed is CG3_HASH_SEED = 705577479, not 0; callers via `hash_value` always
> force a non-zero seed.) (2) If `len == 0` OR `data == nullptr`, return 0
> immediately. (3) `rem = len & 3` (0..3 trailing bytes); `len >>= 2`
> (number of 4-byte blocks). (4) Main loop, once per block:
> `hash += get16bits(data)`; `tmp = (get16bits(data+2) << 11) ^ hash`;
> `hash = (hash << 16) ^ tmp`; `data += 4` (= 2*sizeof(uint16_t));
> `hash += hash >> 11`. Here `get16bits(d)` reads a 16-bit value from two
> bytes: on the "known LE" compilers it is `*(const uint16_t*)d` (a raw
> host-endian uint16 load), and the portable fallback is
> `(UI32(d[1]) << 8) + UI32(d[0])` (explicit LITTLE-ENDIAN assembly of
> `d[0]` as low byte, `d[1]` as high byte). Both agree on little-endian
> hosts; the portable form defines the byte-order contract as
> little-endian. (5) End cases on `rem`:
>  - rem==3: `hash += get16bits(data)`; `hash ^= hash << 16`;
>    `hash ^= data[sizeof(uint16_t)] << 18` (i.e. the 3rd remaining byte
>    `data[2]`, as a `char`, shifted left 18 and XORed);
>    `hash += hash >> 11`.
>  - rem==2: `hash += get16bits(data)`; `hash ^= hash << 11`;
>    `hash += hash >> 17`.
>  - rem==1: `hash += *data` (single `char` byte); `hash ^= hash << 10`;
>    `hash += hash >> 1`.
> (6) Avalanche: `hash ^= hash << 3; hash += hash >> 5; hash ^= hash << 4;
> hash += hash >> 17; hash ^= hash << 25; hash += hash >> 6`. (7)
> Reserved-value remap: if `hash == 0` OR `hash == 0xFFFFFFFF` OR
> `hash == 0xFFFFFFFE`, set `hash = CG3_HASH_SEED`. (8) Return `hash`.
> BYTE-PARITY NOTES the port must honor: input is hashed as BYTES (not
> UTF-16 code units — there is a separate `SuperFastHash(const UChar*)`
> overload, not specced here, that processes 16-bit code units with
> `rem = len & 1`). Because `data` is `const char*` (signed char on most
> targets), the single-byte reads `*data` (rem==1) and `data[2]` (rem==3)
> sign-extend bytes >= 0x80 to negative ints before the shift/add — the
> Rust port must replicate this signed-char treatment (and the strictly
> little-endian get16bits) to reproduce identical hashes.

> [spec:cg3:def:inlines.cg3.swapper]
> class swapper {
>   bool cond;
>   T& a;
>   T& b;
> }

> [spec:cg3:def:inlines.cg3.swapper-false]
> class swapper_false {
>   bool val;
>   swapper<bool> swp;
> }

> [spec:cg3:def:inlines.cg3.swapper-false.swapper-false-fn]
> swapper_false(bool cond, bool& b)

> [spec:cg3:sem:inlines.cg3.swapper-false.swapper-false-fn]
> Constructor of a scoped guard that temporarily forces a bool `b` to false.
> Member init order (matters): first `val` is initialized to `false`, then
> member `swp` (a `swapper<bool>`) is constructed with `(cond, val, b)`.
> By swapper's semantics, if `cond` is true it swaps `val` and `b` on
> construction — so `b` receives `val`'s value (false) and `val` receives
> b's old value; and swapper's destructor swaps them back when the
> `swapper_false` is destroyed, restoring `b`. Net effect: while a
> `swapper_false` object (constructed with `cond == true`) is alive, `b` is
> held at `false`, and `b` is restored to its original value at end of
> scope. If `cond` is false, no swap ever occurs and `b` is untouched.

> [spec:cg3:def:inlines.cg3.swapper.swapper-fn]
> swapper(bool cond, T& a, T& b)

> [spec:cg3:sem:inlines.cg3.swapper.swapper-fn]
> Constructor of the RAII conditional-swap guard. Stores `cond` and binds
> references to `a` and `b`. If `cond` is true, immediately performs
> `std::swap(a, b)`. The destructor (symmetrically) performs `std::swap(a, b)`
> again iff `cond` is true, undoing the swap. So while the `swapper` is
> alive, `a` and `b` are swapped iff `cond`; on destruction the original
> values are restored. If `cond` is false, nothing is ever swapped.

> [spec:cg3:def:inlines.cg3.uncond-swap]
> class uncond_swap {
>   T& a_;
>   T b_;
> }

> [spec:cg3:def:inlines.cg3.uncond-swap.uncond-swap-fn]
> uncond_swap(T& a, T b)

> [spec:cg3:sem:inlines.cg3.uncond-swap.uncond-swap-fn]
> Constructor of an RAII UNCONDITIONAL swap guard. `b` is taken BY VALUE (a
> copy). Members: `a_` is a reference bound to the caller's `a`; `b_` is
> initialized from the copy `b`. The constructor then does
> `std::swap(a_, b_)`, so afterward `a` (through `a_`) holds the passed
> value and `b_` holds `a`'s original value. The destructor does
> `std::swap(a_, b_)` again, restoring `a` to its original value at end of
> scope. Always swaps (no condition). Because `b` was passed by value, the
> caller's original argument to `b` is not affected — only `a` is
> temporarily overwritten.

> [spec:cg3:def:inlines.cg3.usv-fn]
> inline UStringView USV(UnicodeString& str)

> [spec:cg3:sem:inlines.cg3.usv-fn]
> Builds a `UStringView` (a `basic_string_view<UChar>`) over an ICU
> `UnicodeString`'s internal buffer, without copying. Returns
> `UStringView(str.getTerminatedBuffer(), str.length())`:
> `getTerminatedBuffer()` returns a pointer to the NUL-terminated internal
> UTF-16 buffer (this call may mutate/reallocate `str` internally to ensure
> termination), and `length()` gives the number of UChar code units. The
> returned view is valid only while `str` is unmodified and alive. (A
> sibling overload `USV(UString&)` simply returns `UStringView(str)`.)

> [spec:cg3:def:inlines.cg3.write-be-fn]
> inline void writeBE(std::ostream& stream, double value)

> [spec:cg3:sem:inlines.cg3.write-be-fn]
> Full specialization of `writeBE` for `double`, encoding it as a big-endian
> mantissa+exponent pair. Steps: (1) `int exp = 0`. (2)
> `frexp(value, &exp)` returns the fraction `m` in [0.5, 1) (or 0 for
> value 0) with `value = m * 2^exp`, storing the exponent in `exp`. (3)
> `mant64 = UI64(SI64(DBL(INT64_MAX) * m))` — multiply INT64_MAX (as double)
> by `m`, cast to int64 (`SI64`), then reinterpret those bits as uint64
> (`UI64`). (4) `exp32 = UI32(exp)`. (5) `writeBE(stream, mant64)` writes
> the uint64 big-endian (8 bytes) via the generic writeBE template
> (`be::native_to_big` + `writeRaw`). (6) `writeBE(stream, exp32)` writes
> the uint32 big-endian (4 bytes). Total output: 12 bytes = 8-byte BE
> mantissa then 4-byte BE exponent. This is the exact inverse of
> `readBE<double>`. (The generic `writeBE<T>` it specializes does
> `value = be::native_to_big(value); writeRaw(stream, value)`.)

> [spec:cg3:def:inlines.cg3.write-le-fn]
> inline void writeLE(S& stream, T value)

> [spec:cg3:sem:inlines.cg3.write-le-fn]
> Template `<S, T>`. Converts `value` (taken by value; a local copy) from
> host byte order to little-endian via `value = be::native_to_little(value)`,
> then writes its `sizeof(T)` raw bytes to `stream` via `writeRaw`. On a
> little-endian host `native_to_little` is a no-op; on a big-endian host it
> byte-swaps. Writes exactly `sizeof(T)` bytes in little-endian order.
> Returns void.

> [spec:cg3:def:inlines.cg3.write-raw-fn]
> inline void writeRaw(S& stream, const T& value)

> [spec:cg3:sem:inlines.cg3.write-raw-fn]
> Writes the raw object representation of `value`:
> `stream.write(reinterpret_cast<const char*>(&value), sizeof(T))` — emits
> `sizeof(T)` bytes exactly as they sit in memory, in HOST byte order, with
> no endian conversion or formatting. Returns void.

> [spec:cg3:def:inlines.cg3.write-utf8-le-fn]
> inline void writeUTF8_LE(std::ostream& output, const UChar* str, size_t len = 0)

> [spec:cg3:sem:inlines.cg3.write-utf8-le-fn]
> Encodes a UTF-16 string to UTF-8 and writes it length-prefixed with a
> LITTLE-ENDIAN 16-bit prefix. Steps: (1) If `len == 0`, set
> `len = u_strlen(str)` (code-unit length up to the NUL). (2) Allocate
> `std::vector<char> buffer(len * 4)` (a safe upper bound: up to 4 UTF-8
> bytes per source unit). (3) `u_strToUTF8(&buffer[0], SI32(len*4 - 1),
> &olen, str, SI32(len), &status)` converts UTF-16 -> UTF-8; destination
> capacity passed is `len*4 - 1`, source length is `len`, and `olen`
> receives the number of UTF-8 bytes produced. (4) `cs = UI16(olen)` —
> TRUNCATE the byte count to a 16-bit unsigned value. (5) `writeLE(output,
> cs)` writes that uint16 length little-endian (2 bytes). (6)
> `output.write(&buffer[0], cs)` writes `cs` UTF-8 bytes. On-disk format:
> 2-byte LE byte-count then that many UTF-8 bytes. QUIRK/LIMIT: the length
> prefix is only 16 bits, so a string whose UTF-8 form exceeds 65535 bytes
> wraps (`UI16` truncation) and is written/read incorrectly. Also, if `str`
> is genuinely empty so `len` stays 0, `SI32(len*4 - 1)` underflows to -1;
> the source length is 0 so nothing meaningful is written. Overloads for
> `UString`/`UStringView` forward to this using `.data()`/`.size()`.

> [spec:cg3:def:inlines.cg3.write-utf8-raw-fn]
> inline void writeUTF8_Raw(std::ostream& output, const UChar* str, size_t len = 0)

> [spec:cg3:sem:inlines.cg3.write-utf8-raw-fn]
> Identical to `writeUTF8_LE` except the length prefix is written RAW (host
> byte order) instead of little-endian. Steps: (1) if `len == 0`,
> `len = u_strlen(str)`. (2) `std::vector<char> buffer(len * 4)`. (3)
> `u_strToUTF8(&buffer[0], SI32(len*4 - 1), &olen, str, SI32(len), &status)`
> -> `olen` UTF-8 bytes. (4) `cs = UI16(olen)` (16-bit truncation, same
> 65535-byte limit quirk). (5) `writeRaw(output, cs)` writes the uint16
> count in NATIVE/host byte order (the only difference from _LE, which uses
> `writeLE`). (6) `output.write(&buffer[0], cs)`. So the byte-count prefix
> is host-endian-dependent here. An overload taking `const UString&`
> forwards with `.data()`/`.size()`.

> [spec:cg3:def:inlines.dbl-fn]
> constexpr inline double DBL(T t)

> [spec:cg3:sem:inlines.dbl-fn]
> constexpr. Returns `static_cast<double>(t)` — converts `t` to `double`
> using the standard arithmetic/pointer-to-arithmetic conversion. Thin
> named wrapper around the cast.

> [spec:cg3:def:inlines.si32-fn]
> constexpr inline int32_t SI32(T t)

> [spec:cg3:sem:inlines.si32-fn]
> constexpr. Returns `static_cast<int32_t>(t)` — narrows/converts `t` to a
> signed 32-bit integer. For out-of-range integer inputs this is the usual
> modular/implementation-defined conversion to `int32_t` (in practice
> truncation to the low 32 bits, interpreted as two's-complement).

> [spec:cg3:def:inlines.si64-fn]
> constexpr inline int64_t SI64(T t)

> [spec:cg3:sem:inlines.si64-fn]
> constexpr. Returns `static_cast<int64_t>(t)` — converts `t` to a signed
> 64-bit integer (truncating a wider/float value, or sign/zero-extending a
> narrower one, per standard conversion rules).

> [spec:cg3:def:inlines.si8-fn]
> constexpr inline int8_t SI8(T t)

> [spec:cg3:sem:inlines.si8-fn]
> constexpr. Returns `static_cast<int8_t>(t)` — narrows `t` to a signed
> 8-bit integer (keeps the low 8 bits, interpreted as two's-complement).

> [spec:cg3:def:inlines.ui16-fn]
> constexpr inline uint16_t UI16(T t)

> [spec:cg3:sem:inlines.ui16-fn]
> constexpr. Returns `static_cast<uint16_t>(t)` — narrows `t` to an unsigned
> 16-bit integer, i.e. the value modulo 2^16 (low 16 bits).

> [spec:cg3:def:inlines.ui32-fn]
> constexpr inline uint32_t UI32(T t)

> [spec:cg3:sem:inlines.ui32-fn]
> constexpr. Returns `static_cast<uint32_t>(t)` — narrows/converts `t` to an
> unsigned 32-bit integer, i.e. the value modulo 2^32 (low 32 bits).

> [spec:cg3:def:inlines.ui64-fn]
> constexpr inline uint64_t UI64(T t)

> [spec:cg3:sem:inlines.ui64-fn]
> constexpr. Returns `static_cast<uint64_t>(t)` — converts `t` to an
> unsigned 64-bit integer, i.e. the value modulo 2^64 (zero-extending
> narrower unsigned inputs; reinterpreting negative signed inputs as their
> two's-complement modular value).

> [spec:cg3:def:inlines.ui8-fn]
> constexpr inline uint8_t UI8(T t)

> [spec:cg3:sem:inlines.ui8-fn]
> constexpr. Returns `static_cast<uint8_t>(t)` — narrows `t` to an unsigned
> 8-bit integer, i.e. the value modulo 2^8 (low 8 bits).

> [spec:cg3:def:inlines.uiz-fn]
> constexpr inline size_t UIZ(T t)

> [spec:cg3:sem:inlines.uiz-fn]
> constexpr. Returns `static_cast<size_t>(t)` — converts `t` to the
> platform's `size_t` (unsigned, pointer-width, typically 64-bit) via the
> standard conversion.

> [spec:cg3:def:inlines.voidp-fn]
> constexpr inline void* VOIDP(T t)

> [spec:cg3:sem:inlines.voidp-fn]
> constexpr. Returns `static_cast<void*>(t)` — casts a (typically pointer)
> value `t` to `void*`. Thin named wrapper around the cast, used to erase a
> pointer's type.
