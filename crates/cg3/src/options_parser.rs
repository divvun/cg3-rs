//! Port of `src/options_parser.hpp` (`parse_opts` / `parse_opts_env`).
//!
//! Tokenizes a command line embedded in text (an env var, or a grammar
//! `CMDARGS` directive) into an `argv` vector and feeds it to `u_parseArgs` to
//! populate a `UOption` table.
//!
//! ## Genericity (NOTE)
//! The C++ functions are `template<typename Opts>` so they work with both the
//! vislcg3 (`Options`) and cg-conv (`OptionsConv`) tables. Both tables are
//! `std::array<UOption, N>` over the *same* `UOption` type, so the port collapses
//! the template into a plain `&mut [UOption]` slice (any `[UOption; N]` coerces).
//!
//! ## Buffer representation & the trailing-NUL quirk (NOTE)
//! The C++ `parse_opts(char* p, ...)` mutates a NUL-terminated buffer in place
//! (writing NULs to terminate tokens). It is modelled here as `&mut [UChar]`
//! (`char` buffer) plus a `usize` cursor, exactly like the crate's other
//! buffer-scanning code. See [`parse_opts`] for the one-byte-past-terminator
//! quirk this faithfully reproduces.
//!
//! ## `u_parseArgs` routing (NOTE / reconcile)
//! The live `parse_opts` calls the `u_parseArgs` from the vendored
//! `include/uoptions.hpp` (out of scope). We route to
//! [`crate::icu_uoptions::u_parseArgs`], whose ported logic is identical to the
//! live version (the dead `optionFn` block aside). RECONCILE once uoptions.hpp
//! is ported.

use crate::icu_uoptions::u_parseArgs;
use crate::inlines::{isspace, skipto, skiptows};
use crate::options::UOption;
use crate::types::UChar;

/// Extracts the token `p[n .. cursor]` (the C string starting at `n`, up to the
/// NUL just written at `cursor`) as an owned, NUL-free `Vec<char>` — the form
/// [`u_parseArgs`] consumes for each `argv` element.
#[inline]
fn token(p: &[UChar], n: usize, cursor: usize) -> Vec<UChar> {
    p[n..cursor].to_vec()
}

// [spec:cg3:def:options-parser.options.parse-opts-fn]
// [spec:cg3:sem:options-parser.options.parse-opts-fn]
//
// QUIRK (faithfulness): when the final token runs to the end of the buffer,
// `skiptows` stops on the terminating NUL, `p[pos] = '\0'` rewrites it, and the
// trailing `pos += 1` advances ONE BYTE PAST the terminator; the outer
// `while p[pos] != '\0'` then dereferences that byte. Callers must therefore
// pass a buffer with an EXTRA trailing NUL (so the read lands on a second NUL
// and the loop stops cleanly) — see `parse_opts_env`, which appends it, and the
// C++ grammar-cmdargs callers, which likewise append a `0`. In this Rust port
// the "one byte past" access is a slice index: with the required extra NUL it
// reads that NUL; without it, it panics (index out of bounds) — the faithful
// analogue of the C++ out-of-bounds read.
pub fn parse_opts(p: &mut [UChar], where_: &mut [UOption]) {
    let mut argv: Vec<Vec<UChar>> = vec![Vec::new()]; // 0th element is the program name
    let mut pos: usize = 0;
    while p[pos] != '\0' {
        while p[pos] != '\0' && isspace(p[pos]) {
            pos += 1;
        }
        if p[pos] == '-' {
            let n = pos;
            skiptows(p, &mut pos, '\0', false, false);
            p[pos] = '\0';
            argv.push(token(p, n, pos));
            pos += 1;
        }
        else if p[pos] == '"' {
            pos += 1;
            let n = pos;
            skipto(p, &mut pos, '"');
            p[pos] = '\0';
            argv.push(token(p, n, pos));
            pos += 1;
        }
        else if p[pos] == '\'' {
            pos += 1;
            let n = pos;
            skipto(p, &mut pos, '\'');
            p[pos] = '\0';
            argv.push(token(p, n, pos));
            pos += 1;
        }
        else {
            let n = pos;
            skiptows(p, &mut pos, '\0', false, false);
            p[pos] = '\0';
            argv.push(token(p, n, pos));
            pos += 1;
        }
    }
    let n_opts = where_.len() as i32;
    u_parseArgs(argv.len() as i32, &mut argv, n_opts, where_);
}

// [spec:cg3:def:options-parser.options.parse-opts-env-fn]
// [spec:cg3:sem:options-parser.options.parse-opts-env-fn]
//
// Reads env var `which`; if set, parses its value into `where_`. The C++ copies
// the value into a `std::string env` and does `env.push_back(0)` — that ONE
// extra NUL, on top of the string's own readable terminator, is what guards the
// `parse_opts` one-byte-past read (see its QUIRK note). The `std::string`
// implicit terminator is not free in a `Vec<char>`, so BOTH NULs are pushed
// explicitly here (marked below).
pub fn parse_opts_env(which: &str, where_: &mut [UOption]) {
    if let Ok(_env) = std::env::var(which) {
        let mut env: Vec<UChar> = _env.chars().collect();
        env.push('\0'); // std::string's implicit readable NUL terminator (what SKIPTOWS/SKIPTO stop on)
        env.push('\0'); // env.push_back(0): the REQUIRED extra guard NUL the trailing `++p` lands on
        parse_opts(&mut env, where_);
    }
}
