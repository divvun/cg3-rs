//! The JSON reader and writer of the JSONL applicator: RapidJSON's
//! `Document::Parse` with its default flags, and its `Writer`, as the C++
//! uses them.
//!
//! A JSONL line's warnings name RapidJSON's error message and the byte offset
//! it gives, so the reader takes the same steps in the same order and stops
//! where RapidJSON stops. A document keeps every member of an object in the
//! order it was read, a repeated name included: a lookup finds the first, and
//! `"rels"` visits them all. A number keeps only what the applicator asks of
//! it, RapidJSON's `IsUint()`.
//!
//! RapidJSON's reader recurses once per level of nesting; this one keeps its
//! open arrays and objects on a heap stack, and a [`Document`] holds its values
//! in one flat list, so neither reading nor dropping a document recurses
//! (`[spec:cg3:req:robustness.depth-bounded]`).

/// RapidJSON's `ParseErrorCode`, for the errors its default parse can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseErrorCode {
    DocumentEmpty,
    DocumentRootNotSingular,
    ValueInvalid,
    ObjectMissName,
    ObjectMissColon,
    ObjectMissCommaOrCurlyBracket,
    ArrayMissCommaOrSquareBracket,
    StringUnicodeEscapeInvalidHex,
    StringUnicodeSurrogateInvalid,
    StringEscapeInvalid,
    StringMissQuotationMark,
    NumberTooBig,
    NumberMissFraction,
    NumberMissExponent,
}

impl std::fmt::Display for ParseErrorCode {
    /// RapidJSON's `GetParseError_En`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ParseErrorCode::DocumentEmpty => "The document is empty.",
            ParseErrorCode::DocumentRootNotSingular => {
                "The document root must not be followed by other values."
            }
            ParseErrorCode::ValueInvalid => "Invalid value.",
            ParseErrorCode::ObjectMissName => "Missing a name for object member.",
            ParseErrorCode::ObjectMissColon => "Missing a colon after a name of object member.",
            ParseErrorCode::ObjectMissCommaOrCurlyBracket => {
                "Missing a comma or '}' after an object member."
            }
            ParseErrorCode::ArrayMissCommaOrSquareBracket => {
                "Missing a comma or ']' after an array element."
            }
            ParseErrorCode::StringUnicodeEscapeInvalidHex => {
                "Incorrect hex digit after \\u escape in string."
            }
            ParseErrorCode::StringUnicodeSurrogateInvalid => {
                "The surrogate pair in string is invalid."
            }
            ParseErrorCode::StringEscapeInvalid => "Invalid escape character in string.",
            ParseErrorCode::StringMissQuotationMark => {
                "Missing a closing quotation mark in string."
            }
            ParseErrorCode::NumberTooBig => "Number too big to be stored in double.",
            ParseErrorCode::NumberMissFraction => "Miss fraction part in number.",
            ParseErrorCode::NumberMissExponent => "Miss exponent in number.",
        })
    }
}

/// A line that is not one JSON value: the error, and the byte of the line at
/// which RapidJSON reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub code: ParseErrorCode,
    pub offset: usize,
}

enum Node {
    Null,
    Bool,
    /// The number, when RapidJSON's `IsUint()` holds for it.
    Number(Option<u32>),
    String(String),
    Array(Vec<usize>),
    Object(Vec<(String, usize)>),
}

/// One parsed line.
pub(crate) struct Document {
    nodes: Vec<Node>,
    root: usize,
}

impl Document {
    pub(crate) fn root(&self) -> Value<'_> {
        Value {
            doc: self,
            node: self.root,
        }
    }
}

/// A value of a [`Document`].
#[derive(Clone, Copy)]
pub(crate) struct Value<'d> {
    doc: &'d Document,
    node: usize,
}

impl<'d> Value<'d> {
    fn node(self) -> &'d Node {
        &self.doc.nodes[self.node]
    }

    fn at(self, node: usize) -> Value<'d> {
        Value {
            doc: self.doc,
            node,
        }
    }

    pub(crate) fn is_object(self) -> bool {
        matches!(self.node(), Node::Object(_))
    }

    pub(crate) fn as_str(self) -> Option<&'d str> {
        match self.node() {
            Node::String(s) => Some(s),
            _ => None,
        }
    }

    /// RapidJSON's `IsUint()` and `GetUint()`: an integer written without a
    /// fraction or an exponent that fits in 32 bits unsigned, `-0` included.
    pub(crate) fn as_uint(self) -> Option<u32> {
        match self.node() {
            Node::Number(n) => *n,
            _ => None,
        }
    }

    pub(crate) fn elements(self) -> Option<impl Iterator<Item = Value<'d>>> {
        match self.node() {
            Node::Array(items) => Some(items.iter().map(move |&n| self.at(n))),
            _ => None,
        }
    }

    pub(crate) fn members(self) -> Option<impl Iterator<Item = (&'d str, Value<'d>)>> {
        match self.node() {
            Node::Object(members) => Some(
                members
                    .iter()
                    .map(move |(name, n)| (name.as_str(), self.at(*n))),
            ),
            _ => None,
        }
    }

    /// RapidJSON's `FindMember`: the first member named `key`.
    pub(crate) fn member(self, key: &str) -> Option<Value<'d>> {
        self.members()?
            .find(|&(name, _)| name == key)
            .map(|(_, v)| v)
    }
}

/// An array or object the reader is inside: the values read so far, and for
/// an object the name of the member whose value is being read.
enum Open {
    Array(Vec<usize>),
    Object(Vec<(String, usize)>, String),
}

struct Reader<'s> {
    s: &'s [u8],
    pos: usize,
    nodes: Vec<Node>,
}

/// RapidJSON's `Document::Parse(str, length)`: `line` as one JSON value.
pub(crate) fn parse(line: &str) -> Result<Document, ParseError> {
    let mut r = Reader {
        s: line.as_bytes(),
        pos: 0,
        nodes: Vec::new(),
    };
    // The UTF-8 `EncodedInputStream` over a `MemoryStream` skips each byte of
    // a byte order mark on its own, and the offsets still count them.
    for bom in [0xEF, 0xBB, 0xBF] {
        r.consume(bom);
    }
    r.skip_whitespace();
    if r.peek() == 0 {
        return Err(r.error(ParseErrorCode::DocumentEmpty));
    }
    let root = r.value()?;
    r.skip_whitespace();
    if r.peek() != 0 {
        return Err(r.error(ParseErrorCode::DocumentRootNotSingular));
    }
    Ok(Document {
        nodes: r.nodes,
        root,
    })
}

impl Reader<'_> {
    /// `MemoryStream::Peek`: the end of the line reads as NUL, as a NUL in it
    /// does.
    fn peek(&self) -> u8 {
        self.s.get(self.pos).copied().unwrap_or(0)
    }

    fn take(&mut self) -> u8 {
        let c = self.peek();
        if self.pos < self.s.len() {
            self.pos += 1;
        }
        c
    }

    fn consume(&mut self, expect: u8) -> bool {
        if self.peek() == expect {
            self.take();
            true
        } else {
            false
        }
    }

    fn error(&self, code: ParseErrorCode) -> ParseError {
        ParseError {
            code,
            offset: self.pos,
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), b' ' | b'\n' | b'\r' | b'\t') {
            self.take();
        }
    }

    fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// `ParseValue`, with the recursion into arrays and objects unrolled onto
    /// `open`. Each step either finishes a value and hands it to the array or
    /// object it is in, or opens a new one and goes on to its first value.
    fn value(&mut self) -> Result<usize, ParseError> {
        let mut open: Vec<Open> = Vec::new();
        loop {
            let mut done = match self.peek() {
                b'{' => {
                    self.take();
                    self.skip_whitespace();
                    if self.consume(b'}') {
                        self.push(Node::Object(Vec::new()))
                    } else {
                        let name = self.member_name()?;
                        open.push(Open::Object(Vec::new(), name));
                        continue;
                    }
                }
                b'[' => {
                    self.take();
                    self.skip_whitespace();
                    if self.consume(b']') {
                        self.push(Node::Array(Vec::new()))
                    } else {
                        open.push(Open::Array(Vec::new()));
                        continue;
                    }
                }
                b'n' => self.literal(b"ull", Node::Null)?,
                b't' => self.literal(b"rue", Node::Bool)?,
                b'f' => self.literal(b"alse", Node::Bool)?,
                b'"' => {
                    let s = self.string()?;
                    self.push(Node::String(s))
                }
                _ => {
                    let n = self.number()?;
                    self.push(Node::Number(n))
                }
            };
            loop {
                match open.last_mut() {
                    None => return Ok(done),
                    Some(Open::Array(items)) => {
                        items.push(done);
                        self.skip_whitespace();
                        if self.consume(b',') {
                            self.skip_whitespace();
                            break;
                        }
                        if !self.consume(b']') {
                            return Err(self.error(ParseErrorCode::ArrayMissCommaOrSquareBracket));
                        }
                        let items = std::mem::take(items);
                        open.pop();
                        done = self.push(Node::Array(items));
                    }
                    Some(Open::Object(members, name)) => {
                        members.push((std::mem::take(name), done));
                        self.skip_whitespace();
                        match self.peek() {
                            b',' => {
                                self.take();
                                self.skip_whitespace();
                                *name = self.member_name()?;
                                break;
                            }
                            b'}' => {
                                self.take();
                                let members = std::mem::take(members);
                                open.pop();
                                done = self.push(Node::Object(members));
                            }
                            _ => {
                                return Err(
                                    self.error(ParseErrorCode::ObjectMissCommaOrCurlyBracket)
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// `ParseObject`'s steps from a member's name up to its value.
    fn member_name(&mut self) -> Result<String, ParseError> {
        if self.peek() != b'"' {
            return Err(self.error(ParseErrorCode::ObjectMissName));
        }
        let name = self.string()?;
        self.skip_whitespace();
        if !self.consume(b':') {
            return Err(self.error(ParseErrorCode::ObjectMissColon));
        }
        self.skip_whitespace();
        Ok(name)
    }

    /// `ParseNull`, `ParseTrue`, `ParseFalse`: the first letter, then `rest`.
    fn literal(&mut self, rest: &[u8], node: Node) -> Result<usize, ParseError> {
        self.take();
        for &c in rest {
            if !self.consume(c) {
                return Err(self.error(ParseErrorCode::ValueInvalid));
            }
        }
        Ok(self.push(node))
    }

    /// `ParseHex4`. RapidJSON reports a bad digit at the escape's backslash,
    /// not at the digit.
    fn hex4(&mut self, escape: usize) -> Result<u32, ParseError> {
        let mut codepoint = 0;
        for _ in 0..4 {
            let digit = char::from(self.peek()).to_digit(16).ok_or(ParseError {
                code: ParseErrorCode::StringUnicodeEscapeInvalidHex,
                offset: escape,
            })?;
            codepoint = (codepoint << 4) | digit;
            self.take();
        }
        Ok(codepoint)
    }

    /// `ParseString` and `ParseStringToStream`, from the opening quote.
    ///
    /// A `\u` escape of a lone low surrogate is encoded as RapidJSON encodes
    /// it, as three bytes that are not UTF-8, and decoding those gives three
    /// U+FFFD, as the C++'s decoding through ICU does.
    fn string(&mut self) -> Result<String, ParseError> {
        self.take();
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek();
            if c == b'\\' {
                let escape = self.pos;
                self.take();
                let e = self.peek();
                let simple = match e {
                    b'"' => Some(b'"'),
                    b'/' => Some(b'/'),
                    b'\\' => Some(b'\\'),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0C),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                };
                if let Some(byte) = simple {
                    self.take();
                    out.push(byte);
                } else if e == b'u' {
                    self.take();
                    let mut codepoint = self.hex4(escape)?;
                    if (0xD800..=0xDBFF).contains(&codepoint) {
                        if !self.consume(b'\\') || !self.consume(b'u') {
                            return Err(ParseError {
                                code: ParseErrorCode::StringUnicodeSurrogateInvalid,
                                offset: escape,
                            });
                        }
                        let low = self.hex4(escape)?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(ParseError {
                                code: ParseErrorCode::StringUnicodeSurrogateInvalid,
                                offset: escape,
                            });
                        }
                        codepoint = (((codepoint - 0xD800) << 10) | (low - 0xDC00)) + 0x10000;
                    }
                    encode_utf8(codepoint, &mut out);
                } else {
                    return Err(ParseError {
                        code: ParseErrorCode::StringEscapeInvalid,
                        offset: escape,
                    });
                }
            } else if c == b'"' {
                self.take();
                return Ok(match String::from_utf8(out) {
                    Ok(s) => s,
                    Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
                });
            } else if c < 0x20 {
                return Err(self.error(if c == 0 {
                    ParseErrorCode::StringMissQuotationMark
                } else {
                    ParseErrorCode::StringEscapeInvalid
                }));
            } else {
                out.push(self.take());
            }
        }
    }

    /// `ParseNumber` with RapidJSON's default (normal) precision, on a 64-bit
    /// build. Only `IsUint()` is kept of the value; the rest of the steps
    /// decide where RapidJSON stops and which errors it raises.
    fn number(&mut self) -> Result<Option<u32>, ParseError> {
        let start = self.pos;
        let minus = self.consume(b'-');

        let mut i: u32 = 0;
        let mut i64: u64 = 0;
        let mut use_64bit = false;
        let mut significand_digits: i32 = 0;
        let (limit, last) = if minus {
            (214_748_364, b'8')
        } else {
            (429_496_729, b'5')
        };
        if self.peek() == b'0' {
            self.take();
        } else if matches!(self.peek(), b'1'..=b'9') {
            i = u32::from(self.take() - b'0');
            while self.peek().is_ascii_digit() {
                if i >= limit && (i != limit || self.peek() > last) {
                    i64 = u64::from(i);
                    use_64bit = true;
                    break;
                }
                i = i * 10 + u32::from(self.take() - b'0');
                significand_digits += 1;
            }
        } else {
            return Err(self.error(ParseErrorCode::ValueInvalid));
        }

        let mut use_double = false;
        let mut d = 0.0f64;
        if use_64bit {
            let (limit, last) = if minus {
                (0x0CCC_CCCC_CCCC_CCCC, b'8')
            } else {
                (0x1999_9999_9999_9999, b'5')
            };
            while self.peek().is_ascii_digit() {
                if i64 >= limit && (i64 != limit || self.peek() > last) {
                    d = i64 as f64;
                    use_double = true;
                    break;
                }
                i64 = i64 * 10 + u64::from(self.take() - b'0');
                significand_digits += 1;
            }
        }
        if use_double {
            while self.peek().is_ascii_digit() {
                if d >= 1.797_693_134_862_315_7e307 {
                    return Err(ParseError {
                        code: ParseErrorCode::NumberTooBig,
                        offset: start,
                    });
                }
                d = d * 10.0 + f64::from(self.take() - b'0');
            }
        }

        let mut exp_frac: i64 = 0;
        if self.consume(b'.') {
            if !self.peek().is_ascii_digit() {
                return Err(self.error(ParseErrorCode::NumberMissFraction));
            }
            if !use_double {
                if !use_64bit {
                    i64 = u64::from(i);
                }
                while self.peek().is_ascii_digit() {
                    if i64 > 0x1F_FFFF_FFFF_FFFF {
                        break;
                    }
                    i64 = i64 * 10 + u64::from(self.take() - b'0');
                    exp_frac -= 1;
                    if i64 != 0 {
                        significand_digits += 1;
                    }
                }
                d = i64 as f64;
                use_double = true;
            }
            while self.peek().is_ascii_digit() {
                if significand_digits < 17 {
                    d = d * 10.0 + f64::from(self.take() - b'0');
                    exp_frac -= 1;
                    if d > 0.0 {
                        significand_digits += 1;
                    }
                } else {
                    self.take();
                }
            }
        }

        if self.consume(b'e') || self.consume(b'E') {
            use_double = true;
            let exp_minus = if self.consume(b'+') {
                false
            } else {
                self.consume(b'-')
            };
            if !self.peek().is_ascii_digit() {
                return Err(self.error(ParseErrorCode::NumberMissExponent));
            }
            let mut exp = i64::from(self.take() - b'0');
            let max_exp = 308 - exp_frac;
            while self.peek().is_ascii_digit() {
                exp = exp * 10 + i64::from(self.take() - b'0');
                if exp_minus {
                    if exp >= 214_748_364 {
                        while self.peek().is_ascii_digit() {
                            self.take();
                        }
                    }
                } else if exp > max_exp {
                    return Err(ParseError {
                        code: ParseErrorCode::NumberTooBig,
                        offset: start,
                    });
                }
            }
        }

        // A double, an `Int64` or a `Uint64` is not `IsUint()`; an `Int` is
        // when it is not negative, which leaves `-0`.
        Ok(if use_double || use_64bit {
            None
        } else if minus {
            (i == 0).then_some(0)
        } else {
            Some(i)
        })
    }
}

/// RapidJSON's `UTF8::Encode`, which encodes a surrogate code point as any
/// other.
fn encode_utf8(codepoint: u32, out: &mut Vec<u8>) {
    // Each `as u8` keeps the low bits a UTF-8 byte carries.
    if codepoint <= 0x7F {
        out.push(codepoint as u8);
    } else if codepoint <= 0x7FF {
        out.push(0xC0 | (codepoint >> 6) as u8);
        out.push(0x80 | (codepoint & 0x3F) as u8);
    } else if codepoint <= 0xFFFF {
        out.push(0xE0 | (codepoint >> 12) as u8);
        out.push(0x80 | ((codepoint >> 6) & 0x3F) as u8);
        out.push(0x80 | (codepoint & 0x3F) as u8);
    } else {
        out.push(0xF0 | (codepoint >> 18) as u8);
        out.push(0x80 | ((codepoint >> 12) & 0x3F) as u8);
        out.push(0x80 | ((codepoint >> 6) & 0x3F) as u8);
        out.push(0x80 | (codepoint & 0x3F) as u8);
    }
}

// [spec:cg3:def:jsonl-applicator.cg3.write-json-line-fn]
// [spec:cg3:sem:jsonl-applicator.cg3.write-json-line-fn]
/// RapidJSON's `Writer` into a `StringBuffer`: one JSON value written as it
/// is built, with no whitespace between tokens, so object members come out
/// in the order they are added. Strings are escaped as the `Writer` escapes
/// them, which serde_json's serialiser does not do: control characters as
/// `\u00XX` with uppercase hex digits.
pub(crate) struct Writer {
    out: String,
    /// For each object and array the writer is inside, whether it has a
    /// value yet.
    has_value: Vec<bool>,
    /// A member name was just written, so its value takes no comma.
    after_name: bool,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Writer {
            out: String::new(),
            has_value: Vec::new(),
            after_name: false,
        }
    }

    fn separate(&mut self) {
        if std::mem::take(&mut self.after_name) {
            return;
        }
        if let Some(has_value) = self.has_value.last_mut() {
            if *has_value {
                self.out.push(',');
            }
            *has_value = true;
        }
    }

    pub(crate) fn start_object(&mut self) {
        self.separate();
        self.out.push('{');
        self.has_value.push(false);
    }

    pub(crate) fn end_object(&mut self) {
        self.has_value.pop();
        self.out.push('}');
    }

    pub(crate) fn start_array(&mut self) {
        self.separate();
        self.out.push('[');
        self.has_value.push(false);
    }

    pub(crate) fn end_array(&mut self) {
        self.has_value.pop();
        self.out.push(']');
    }

    /// The name of the next member of the object being written.
    pub(crate) fn name(&mut self, name: &str) {
        self.separate();
        self.write_string(name);
        self.out.push(':');
        self.after_name = true;
    }

    pub(crate) fn string(&mut self, s: &str) {
        self.separate();
        self.write_string(s);
    }

    pub(crate) fn uint(&mut self, n: u32) {
        self.separate();
        self.out.push_str(&n.to_string());
    }

    pub(crate) fn string_member(&mut self, name: &str, s: &str) {
        self.name(name);
        self.string(s);
    }

    /// `name` with an array of the strings `items`.
    pub(crate) fn strings_member<'s>(
        &mut self,
        name: &str,
        items: impl IntoIterator<Item = &'s str>,
    ) {
        self.name(name);
        self.start_array();
        for item in items {
            self.string(item);
        }
        self.end_array();
    }

    /// C++ `write_json_line`: the value written, then a newline. A written
    /// value holds no raw newline, so this writes exactly one line. Does not
    /// flush.
    pub(crate) fn write_line<W: std::io::Write>(mut self, output: &mut W) {
        self.out.push('\n');
        let _ = output.write_all(self.out.as_bytes());
    }

    #[cfg(test)]
    fn finish(self) -> String {
        self.out
    }

    /// `Writer::WriteString`: characters from U+0080 up are written as they
    /// are, `/` and U+007F too.
    fn write_string(&mut self, s: &str) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.out.push('"');
        for c in s.chars() {
            match c {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\u{8}' => self.out.push_str("\\b"),
                '\t' => self.out.push_str("\\t"),
                '\n' => self.out.push_str("\\n"),
                '\u{c}' => self.out.push_str("\\f"),
                '\r' => self.out.push_str("\\r"),
                c if u32::from(c) < 0x20 => {
                    let b = u32::from(c) as usize;
                    self.out.push_str("\\u00");
                    self.out.push(char::from(HEX[b >> 4]));
                    self.out.push(char::from(HEX[b & 0xF]));
                }
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(line: &str) -> (String, usize) {
        match parse(line) {
            Err(e) => (e.code.to_string(), e.offset),
            Ok(_) => panic!("{line} parsed"),
        }
    }

    // Each error RapidJSON's default parse raises, at the offset the C++
    // reports for it.
    #[test]
    fn errors_have_rapidjson_messages_and_offsets() {
        for (line, message, offset) in [
            ("", "The document is empty.", 0),
            ("\u{FEFF}", "The document is empty.", 3),
            ("\0{}", "The document is empty.", 0),
            (
                "{} x",
                "The document root must not be followed by other values.",
                3,
            ),
            ("{}\0x", "", 0),
            ("x", "Invalid value.", 0),
            ("-x", "Invalid value.", 1),
            ("nul", "Invalid value.", 3),
            ("\u{0B}{}", "Invalid value.", 0),
            ("{", "Missing a name for object member.", 1),
            (
                "{\"a\" 1}",
                "Missing a colon after a name of object member.",
                5,
            ),
            (
                "{\"a\":1 \"b\"}",
                "Missing a comma or '}' after an object member.",
                7,
            ),
            ("[1 2]", "Missing a comma or ']' after an array element.", 3),
            (
                "\"\\u12G4\"",
                "Incorrect hex digit after \\u escape in string.",
                1,
            ),
            ("\"\\uD800\"", "The surrogate pair in string is invalid.", 1),
            (
                "\"\\uD800\\u0041\"",
                "The surrogate pair in string is invalid.",
                1,
            ),
            ("\"\\q\"", "Invalid escape character in string.", 1),
            ("\"a\tb\"", "Invalid escape character in string.", 2),
            ("\"abc", "Missing a closing quotation mark in string.", 4),
            ("\"a\0\"", "Missing a closing quotation mark in string.", 2),
            ("1e309", "Number too big to be stored in double.", 0),
            (
                "[0.001e310, 1e309]",
                "Number too big to be stored in double.",
                12,
            ),
            ("1.", "Miss fraction part in number.", 2),
            ("1e+", "Miss exponent in number.", 3),
        ] {
            if message.is_empty() {
                assert!(parse(line).is_ok(), "{line:?}");
            } else {
                assert_eq!(error(line), (message.to_string(), offset), "{line:?}");
            }
        }
    }

    // Only an integer without a fraction or an exponent that fits in 32 bits
    // unsigned is `IsUint()`.
    #[test]
    fn as_uint_follows_rapidjson_is_uint() {
        for (number, uint) in [
            ("0", Some(0)),
            ("-0", Some(0)),
            ("7", Some(7)),
            ("4294967295", Some(u32::MAX)),
            ("4294967296", None),
            ("18446744073709551616", None),
            ("-1", None),
            ("1.0", None),
            ("1e2", None),
            ("true", None),
            ("\"1\"", None),
        ] {
            let doc = parse(number).unwrap();
            assert_eq!(doc.root().as_uint(), uint, "{number}");
        }
    }

    // A repeated member name is kept where it was read; a lookup finds the
    // first.
    #[test]
    fn repeated_members_are_kept_in_order() {
        let doc = parse("{\"r\":[1],\"s\":2,\"r\":[3]}").unwrap();
        let root = doc.root();
        let names: Vec<&str> = root.members().unwrap().map(|(name, _)| name).collect();
        assert_eq!(names, ["r", "s", "r"]);
        let first: Vec<Option<u32>> = root
            .member("r")
            .unwrap()
            .elements()
            .unwrap()
            .map(Value::as_uint)
            .collect();
        assert_eq!(first, [Some(1)]);
    }

    // Escapes decode as RapidJSON decodes them; a lone low surrogate comes out
    // as three U+FFFD, as the C++'s ICU decoding gives.
    #[test]
    fn string_escapes_decode_as_in_the_cpp() {
        let doc = parse(r#""\"\\\/\b\f\n\r\t\u00e9\u0000\ud83d\ude00\udc00x""#).unwrap();
        assert_eq!(
            doc.root().as_str(),
            Some("\"\\/\u{8}\u{c}\n\r\té\0😀\u{FFFD}\u{FFFD}\u{FFFD}x")
        );
    }

    // Strings are escaped as RapidJSON's `Writer` escapes them: control
    // characters other than the five with short escapes as `\u00XX` with
    // uppercase hex, everything from U+0020 up but `"` and `\` as it is.
    // [spec:cg3:sem:jsonl-applicator.cg3.write-json-line-fn/test]
    #[test]
    fn strings_are_escaped_as_rapidjson_writes_them() {
        let mut w = Writer::new();
        w.start_object();
        w.string_member(
            "z",
            "\0\u{1}\u{8}\t\n\u{b}\u{c}\r\u{1f} \"\\/\u{7f}é\u{2028}😀",
        );
        w.name("n");
        w.start_array();
        w.uint(0);
        w.uint(u32::MAX);
        w.end_array();
        w.name("o");
        w.start_object();
        w.end_object();
        w.end_object();
        assert_eq!(
            w.finish(),
            "{\"z\":\"\\u0000\\u0001\\b\\t\\n\\u000B\\f\\r\\u001F \\\"\\\\/\u{7f}é\u{2028}😀\",\"n\":[0,4294967295],\"o\":{}}"
        );
    }

    // What the writer writes, the reader reads back as it was.
    #[test]
    fn written_strings_read_back_unchanged() {
        let text: String = (0u32..0x250).filter_map(char::from_u32).collect();
        let mut w = Writer::new();
        w.start_object();
        w.string_member("t", &text);
        w.end_object();
        let line = w.finish();
        let doc = parse(&line).unwrap();
        assert_eq!(
            doc.root().member("t").unwrap().as_str(),
            Some(text.as_str())
        );
    }

    // Nesting deeper than any stack would take reads and drops.
    // [spec:cg3:req:robustness.depth-bounded/test]
    #[test]
    fn deep_nesting_reads_without_recursion() {
        let depth = 200_000;
        let line = format!("{}0{}", "[{\"a\":".repeat(depth), "}]".repeat(depth));
        let doc = parse(&line).unwrap();
        let mut value = doc.root();
        for _ in 0..depth {
            value = value
                .elements()
                .unwrap()
                .next()
                .unwrap()
                .member("a")
                .unwrap();
        }
        assert_eq!(value.as_uint(), Some(0));
    }
}
