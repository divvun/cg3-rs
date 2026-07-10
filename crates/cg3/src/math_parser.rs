//! Port of `src/MathParser.hpp`.
//!
//! A literal, bug-for-bug 1:1 translation of the C++ recursive-descent numeric
//! expression evaluator (spec `docs/spec/port/src/MathParser.md`). Same control
//! flow and names (snake_cased) as the original; the flagged quirks are
//! reproduced rather than fixed (Wave 4 does the idiomatic cleanup).
//!
//! Notes on faithful-but-safe approximations (details at each site):
//! * C++ throws `std::runtime_error(msg)`; here those become
//!   `Err(MathError(msg))` and exception propagation is modelled with `?`.
//!   Callers (e.g. `Tag.cpp`) wrap `eval` in `try/catch`, which maps to a
//!   `match`/`if let Err` — so a `Result` return is the faithful shape.
//! * The C++ `char num[128]` stack buffer, out-of-bounds `vars[]` indexing, and
//!   the read-past-end inside `ux_simplecasecmp` are Undefined Behaviour in
//!   C++. Safe Rust cannot reproduce UB; the closest safe behaviour is used and
//!   annotated inline.
//! * `ISSPACE`, `ISDELIM`, `ISALPHA_C`, `ISDIGIT_C` (inlines.hpp) and
//!   `ux_simplecasecmp` (uextras.hpp) belong to other modules that are not yet
//!   wired into `lib.rs`. To keep this file self-contained and compiling, they
//!   are reimplemented here as private local helpers and deliberately left
//!   without `[spec:...]` annotations (their spec ids belong to those modules).

// The `op`/`temp_token` locals mirror C++ default-initializations (`UChar op =
// 0;`, `UStringView temp_token;`) that are overwritten before their first read.
#![allow(unused_assignments)]

use std::cell::Cell;
use std::f64::consts::PI;

use crate::types::{UChar, UStringView};

/// Error raised by the parser. C++ raised `std::runtime_error(&'static str)`;
/// this carries the same message text and propagates via `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathError(pub &'static str);

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for MathError {}

// [spec:cg3:def:math-parser.cg3.math-parser.type-t]
// C++ `enum type_t:uint8_t { DELIMITER = 1, VARIABLE, NUMBER, FUNCTION }`.
// Stored in the `u8` field `tok_type`, whose "no token" state is 0 (a value
// outside this enum), matching the C++ `char tok_type = 0`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeT {
    Delimiter = 1,
    Variable = 2,
    Number = 3,
    Function = 4,
}

const NUMVARS: usize = 26;

/// The set of characters that terminate an identifier/number scan
/// (`find_first_of(u" +-/*%^=()")` in the original). Note the `%` delimiter is
/// in this stop-set even though no eval level actually consumes it.
const STOP_SET: &str = " +-/*%^=()";

// C `ERANGE`; used to model `strtod`'s overflow signalling. See `MP_ERRNO`.
const ERANGE: i32 = 34;

thread_local! {
    // Faithful-but-safe stand-in for C's (thread-local) `errno`. `c_strtod`
    // sets it to `ERANGE` on overflow and NEVER clears it — reproducing the
    // quirk that `errno` is not cleared before the `ERANGE` test, so once any
    // number overflows every later number parse also throws. (Approximation:
    // real `errno` also reflects unrelated libc calls; here only overflow in
    // `c_strtod` ever sets it, and underflow is not detected.)
    static MP_ERRNO: Cell<i32> = const { Cell::new(0) };
}

fn set_errno(v: i32) {
    MP_ERRNO.with(|e| e.set(v));
}

fn errno() -> i32 {
    MP_ERRNO.with(|e| e.get())
}

// [spec:cg3:def:math-parser.cg3.math-parser]
// C++ class members. `exp_ptr`/`token` are string-views into the expression;
// a lifetime parameter (not present in C++) lets them borrow the `exp` passed
// to `eval`. Consequence: one instance's `eval` calls share a lifetime.
pub struct MathParser<'a> {
    exp_ptr: UStringView<'a>,
    token: UStringView<'a>,
    tok_type: u8,
    vars: [f64; NUMVARS],
    min: f64,
    max: f64,
}

impl<'a> MathParser<'a> {
    // [spec:cg3:def:math-parser.cg3.math-parser.math-parser-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.math-parser-fn]
    // Constructor `MathParser(double min=0, double max=0)`. Rust has no default
    // arguments, so both are required; callers pass `0.0, 0.0` for the default.
    // All other members keep their in-class initializers.
    pub fn new(min: f64, max: f64) -> Self {
        MathParser {
            exp_ptr: "",
            token: "",
            tok_type: 0,
            vars: [0.0; NUMVARS],
            min,
            max,
        }
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-fn]
    pub fn eval(&mut self, exp: UStringView<'a>) -> Result<f64, MathError> {
        let mut result: f64 = 0.0;
        self.exp_ptr = exp;
        self.get_token()?;
        if self.token.is_empty() {
            return Err(MathError("Expression empty"));
        }
        self.eval_assign(&mut result)?;
        if !self.token.is_empty() {
            return Err(MathError("Syntax error"));
        }
        Ok(result)
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-assign-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-assign-fn]
    fn eval_assign(&mut self, result: &mut f64) -> Result<(), MathError> {
        let mut temp_token: UStringView<'a> = "";
        if self.tok_type == TypeT::Variable as u8 {
            let t_ptr: UStringView<'a> = self.exp_ptr;
            temp_token = self.token;
            // Quirk: `slot` is `token[0]-'A'` even for MIN/MAX (first letter
            // 'M' => 12) and for a lowercase single-letter name ('a'-'A' == 32),
            // which then indexes past the 26-slot `vars`. In C++ the OOB index
            // is UB; in safe Rust `vars[slot]` panics instead.
            let slot = first_char(self.token) as i32 - 'A' as i32;
            self.get_token()?;
            if first_char(self.token) != '=' {
                self.exp_ptr = t_ptr;
                self.token = temp_token;
                self.tok_type = TypeT::Variable as u8;
            } else {
                self.get_token()?;
                self.eval_add_sub(result)?;
                self.vars[slot as usize] = *result;
                return Ok(());
            }
        }
        self.eval_add_sub(result)
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-add-sub-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-add-sub-fn]
    fn eval_add_sub(&mut self, result: &mut f64) -> Result<(), MathError> {
        let mut op: UChar = '\0';
        let mut temp: f64 = 0.0;
        self.eval_mul_div(result)?;
        while !self.token.is_empty() && {
            op = first_char(self.token);
            op == '+' || op == '-'
        } {
            self.get_token()?;
            self.eval_mul_div(&mut temp)?;
            match op {
                '-' => *result = *result - temp,
                '+' => *result = *result + temp,
                _ => {}
            }
        }
        Ok(())
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-mul-div-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-mul-div-fn]
    fn eval_mul_div(&mut self, result: &mut f64) -> Result<(), MathError> {
        let mut op: UChar = '\0';
        let mut temp: f64 = 0.0;
        self.eval_exp(result)?;
        while !self.token.is_empty() && {
            op = first_char(self.token);
            op == '*' || op == '/'
        } {
            self.get_token()?;
            self.eval_exp(&mut temp)?;
            match op {
                // Plain IEEE-754 division, no zero check: `/0` yields ±inf/NaN.
                '*' => *result = *result * temp,
                '/' => *result = *result / temp,
                _ => {}
            }
        }
        Ok(())
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-exp-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-exp-fn]
    fn eval_exp(&mut self, result: &mut f64) -> Result<(), MathError> {
        let mut temp: f64 = 0.0;
        self.eval_unary(result)?;
        // `while` loop applied left-to-right, so `a^b^c` == `(a^b)^c`
        // (left-associative), unlike conventional right-associative `^`.
        while !self.token.is_empty() && first_char(self.token) == '^' {
            self.get_token()?;
            self.eval_unary(&mut temp)?;
            *result = result.powf(temp);
        }
        Ok(())
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.eval-unary-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-unary-fn]
    fn eval_unary(&mut self, result: &mut f64) -> Result<(), MathError> {
        let mut op: UChar = '\0';
        if self.tok_type == TypeT::Delimiter as u8
            && (first_char(self.token) == '+' || first_char(self.token) == '-')
        {
            op = first_char(self.token);
            self.get_token()?;
        }
        self.eval_func(result)?;
        if op == '-' {
            *result = -*result;
        }
        Ok(())
    }

    // Process a function, a parenthesized expression, a value or a variable
    // [spec:cg3:def:math-parser.cg3.math-parser.eval-func-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.eval-func-fn]
    fn eval_func(&mut self, result: &mut f64) -> Result<(), MathError> {
        let isfunc = self.tok_type == TypeT::Function as u8;
        let mut temp_token: UStringView<'a> = "";
        if isfunc {
            temp_token = self.token;
            self.get_token()?;
        }
        if first_char(self.token) == '(' {
            self.get_token()?;
            self.eval_add_sub(result)?;
            if first_char(self.token) != ')' {
                return Err(MathError("Unbalanced parentheses"));
            }
            if isfunc {
                if ux_simplecasecmp(temp_token, "SIN") {
                    *result = (PI / 180.0 * *result).sin();
                } else if ux_simplecasecmp(temp_token, "COS") {
                    *result = (PI / 180.0 * *result).cos();
                } else if ux_simplecasecmp(temp_token, "TAN") {
                    *result = (PI / 180.0 * *result).tan();
                } else if ux_simplecasecmp(temp_token, "ASIN") {
                    *result = 180.0 / PI * result.asin();
                } else if ux_simplecasecmp(temp_token, "ACOS") {
                    *result = 180.0 / PI * result.acos();
                } else if ux_simplecasecmp(temp_token, "ATAN") {
                    *result = 180.0 / PI * result.atan();
                } else if ux_simplecasecmp(temp_token, "SINH") {
                    *result = result.sinh();
                } else if ux_simplecasecmp(temp_token, "COSH") {
                    *result = result.cosh();
                } else if ux_simplecasecmp(temp_token, "TANH") {
                    *result = result.tanh();
                } else if ux_simplecasecmp(temp_token, "ASINH") {
                    *result = result.asinh();
                } else if ux_simplecasecmp(temp_token, "ACOSH") {
                    *result = result.acosh();
                } else if ux_simplecasecmp(temp_token, "ATANH") {
                    *result = result.atanh();
                } else if ux_simplecasecmp(temp_token, "LN") {
                    *result = result.ln();
                } else if ux_simplecasecmp(temp_token, "LOG") {
                    *result = result.log10();
                } else if ux_simplecasecmp(temp_token, "EXP") {
                    *result = result.exp();
                } else if ux_simplecasecmp(temp_token, "SQRT") {
                    *result = result.sqrt();
                } else if ux_simplecasecmp(temp_token, "SQR") {
                    *result = *result * *result;
                } else if ux_simplecasecmp(temp_token, "ROUND") {
                    *result = result.round();
                } else if ux_simplecasecmp(temp_token, "FLOOR") {
                    *result = result.floor();
                } else {
                    return Err(MathError("Unknown function"));
                }
            }
            self.get_token()?;
        } else if self.tok_type == TypeT::Variable as u8 {
            if ux_simplecasecmp(self.token, "MIN") {
                *result = self.min;
            } else if ux_simplecasecmp(self.token, "MAX") {
                *result = self.max;
            } else {
                // `vars[token[0]-'A']` assumes an uppercase A-Z letter; a
                // lowercase single-letter name indexes out of bounds. C++: UB;
                // safe Rust: panics on OOB.
                *result = self.vars[(first_char(self.token) as i32 - 'A' as i32) as usize];
            }
            self.get_token()?;
            return Ok(());
        } else if self.tok_type == TypeT::Number as u8 {
            // C++ copies the token into a fixed `char num[128]` and null-
            // terminates it; a token of 128+ chars overflows that stack buffer
            // (UB). Safe Rust cannot reproduce the overflow, so the token is
            // parsed directly. `errno` is NOT cleared before the ERANGE test
            // (see `MP_ERRNO`), so this reflects any prior overflow.
            *result = c_strtod(self.token);
            if errno() == ERANGE {
                return Err(MathError("Result did not fit in a double"));
            }
            self.get_token()?;
            return Ok(());
        } else {
            // Any other tok_type (incl. a FUNCTION name not followed by `(`).
            return Err(MathError("Syntax error"));
        }
        Ok(())
    }

    // [spec:cg3:def:math-parser.cg3.math-parser.get-token-fn]
    // [spec:cg3:sem:math-parser.cg3.math-parser.get-token-fn]
    fn get_token(&mut self) -> Result<(), MathError> {
        self.token = self.exp_ptr;
        self.tok_type = 0;
        if self.exp_ptr.is_empty() {
            return Ok(());
        }
        while is_space(first_char(self.exp_ptr)) {
            self.remove_prefix_one();
        }

        let c0 = first_char(self.exp_ptr);
        if is_delim(c0) {
            self.tok_type = TypeT::Delimiter as u8;
            let s = self.exp_ptr;
            let cl = c0.len_utf8();
            self.token = &s[..cl];
            self.exp_ptr = &s[cl..];
        } else if is_alpha_c(c0) {
            let s = self.exp_ptr;
            let idx = find_first_of(s, STOP_SET);
            self.token = &s[..idx];
            self.exp_ptr = &s[idx..];
            while is_space(first_char(self.exp_ptr)) {
                self.remove_prefix_one();
            }
            self.tok_type = if first_char(self.exp_ptr) == '(' {
                TypeT::Function as u8
            } else {
                TypeT::Variable as u8
            };
        } else if is_digit_c(c0) || c0 == '.' {
            let s = self.exp_ptr;
            let idx = find_first_of(s, STOP_SET);
            self.token = &s[..idx];
            self.exp_ptr = &s[idx..];
            self.tok_type = TypeT::Number as u8;
        }
        // else: tok_type stays 0 and token keeps its pre-whitespace-skip value.

        if self.tok_type == TypeT::Variable as u8 {
            if ux_simplecasecmp(self.token, "MIN") || ux_simplecasecmp(self.token, "MAX") {
                // Nothing
            } else if self.token.chars().count() > 1 {
                return Err(MathError(
                    "Variables other than MIN and MAX must be 1 letter",
                ));
            }
        }
        Ok(())
    }

    // Advance `exp_ptr` by one character. C++ `remove_prefix(1)` removes one
    // UTF-16 code unit; every call site here operates on ASCII whitespace or an
    // ASCII delimiter, for which one char == one code unit.
    fn remove_prefix_one(&mut self) {
        let s = self.exp_ptr;
        if let Some(c) = s.chars().next() {
            self.exp_ptr = &s[c.len_utf8()..];
        }
    }
}

// --- Private local reimplementations of inlines.hpp / uextras.hpp helpers. ---
// These live in other modules (not yet wired into lib.rs); duplicated here so
// this file compiles standalone. Intentionally un-annotated (their spec ids
// belong to those modules).

/// Peek the first `char` of `s`, or `'\0'` when empty. Models the C++ reads of
/// `view[0]` on a possibly-empty view, which land on the string's null
/// terminator (`0`) rather than reading past the buffer.
fn first_char(s: &str) -> UChar {
    s.chars().next().unwrap_or('\0')
}

/// `inlines.hpp` `ISDELIM`.
fn is_delim(c: UChar) -> bool {
    c == '(' || c == ')' || c == '+' || c == '-' || c == '*' || c == '/' || c == '^' || c == '%'
        || c == '='
}

/// `inlines.hpp` `ISSPACE`. The `u_isWhitespace` ICU tail (only reachable for
/// code points > 0xFF) is approximated with `char::is_whitespace`.
fn is_space(c: UChar) -> bool {
    let u = c as u32;
    if u <= 0xFF && u != 0x09 && u != 0x0A && u != 0x0D && u != 0x20 && u != 0xA0 {
        return false;
    }
    u == 0x20 || u == 0x09 || u == 0x0A || u == 0x0D || u == 0xA0 || c.is_whitespace()
}

/// `inlines.hpp` `ISALPHA_C`: `(p < 255) && isalpha(p)`. In the C locale
/// `isalpha` matches ASCII A-Z/a-z, approximated with `is_ascii_alphabetic`.
fn is_alpha_c(c: UChar) -> bool {
    (c as u32) < 255 && c.is_ascii_alphabetic()
}

/// `inlines.hpp` `ISDIGIT_C`: `(p < 255) && isdigit(p)`.
fn is_digit_c(c: UChar) -> bool {
    (c as u32) < 255 && c.is_ascii_digit()
}

/// `std::u16string_view::find_first_of(set)` returning a byte index into `s`,
/// or `s.len()` when no char is found (so `&s[..idx]` == whole string, matching
/// `substr(0, npos)`).
fn find_first_of(s: &str, set: &str) -> usize {
    for (i, c) in s.char_indices() {
        if set.contains(c) {
            return i;
        }
    }
    s.len()
}

/// `uextras.hpp` `ux_simplecasecmp(a, b)`: case-insensitive (ASCII, lowercase
/// == uppercase + 32) prefix compare of the first `b.len()` chars, with a
/// trailing-char acceptance check.
///
/// C++ reads `a[i]` for `i` up to `b.size()` and `a[n]` directly from `a`'s
/// buffer, potentially one past `a`'s own length (UB when `a` is shorter). Safe
/// Rust cannot read past the slice: a missing `a[i]` in the compare loop is
/// treated as a mismatch (faithful, because the char following a token is
/// always a delimiter/space/end that never equals a letter), and a missing
/// `a[n]` at the tail is treated as end-of-string (`a[n] == 0` -> match).
///
/// The `u_getCombiningClass(a[n]) == 0` tail is approximated as always true:
/// ICU combining classes are unavailable and are 0 for all ASCII, so — as in
/// C++ — any non-combining trailing char makes the compare succeed once the
/// prefix matches. (This reproduces quirks like `"SINH"` matching `"SIN"`.)
fn ux_simplecasecmp(a: &str, b: &str) -> bool {
    let a_chars: Vec<UChar> = a.chars().collect();
    let b_chars: Vec<UChar> = b.chars().collect();
    let n = b_chars.len();
    for i in 0..n {
        match a_chars.get(i) {
            Some(&ac) => {
                if ac != b_chars[i] && (ac as u32) != (b_chars[i] as u32) + 32 {
                    return false;
                }
            }
            None => return false,
        }
    }
    match a_chars.get(n) {
        None => true,
        Some(&c) => c == '\0' || is_space(c) || is_delim(c) || u_get_combining_class(c) == 0,
    }
}

/// ICU `u_getCombiningClass` is unavailable; combining class is 0 for every
/// ASCII char, which is all that appears in numeric expressions.
fn u_get_combining_class(_c: UChar) -> u8 {
    0
}

/// C `strtod(s, nullptr)`. Parses the leading decimal/scientific numeric
/// prefix, ignoring trailing chars; no conversion yields 0. On overflow to
/// infinity it sets `MP_ERRNO = ERANGE` (matching `strtod`). Approximations:
/// hex floats / `inf` / `nan` are not handled (a NUMBER token can't produce
/// them), and underflow is not flagged ERANGE.
fn c_strtod(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    // strtod skips leading whitespace.
    while i < len && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    let start = i;
    if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut has_digits = false;
    while i < len && bytes[i].is_ascii_digit() {
        i += 1;
        has_digits = true;
    }
    if i < len && bytes[i] == b'.' {
        i += 1;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
            has_digits = true;
        }
    }
    if !has_digits {
        return 0.0;
    }
    if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < len && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if j < len && bytes[j].is_ascii_digit() {
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
        // else: invalid exponent, back up (leave `i` before the 'e').
    }
    match s[start..i].parse::<f64>() {
        Ok(v) => {
            if v.is_infinite() {
                set_errno(ERANGE);
            }
            v
        }
        Err(_) => 0.0,
    }
}
