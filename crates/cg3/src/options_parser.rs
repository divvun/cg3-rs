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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{UOPT_NO_ARG, UOPT_REQUIRES_ARG};

    fn opt(long: &'static str, short: UChar, has_arg: u8) -> UOption {
        UOption {
            long_name: Some(long),
            short_name: short,
            has_arg,
            description: String::new(),
            does_occur: false,
            value: String::new(),
        }
    }

    /// Build the NUL-terminated buffer `parse_opts` scans: the command text,
    /// then the string's own readable terminator NUL, then the extra guard NUL
    /// the trailing one-past read must land on (see the module QUIRK note).
    fn buf(cmd: &str) -> Vec<UChar> {
        let mut v: Vec<UChar> = cmd.chars().collect();
        v.push('\0');
        v.push('\0');
        v
    }

    // `parse_opts` tokenizes an embedded command line (splitting on whitespace,
    // honoring `"`/`'` quotes and `-` option tokens) into an argv and feeds it to
    // u_parseArgs. Drives the dash-token, quoted-token, and bare-token paths plus
    // the trailing-token / extra-NUL quirk. Options populated as a side effect.
    // [spec:cg3:sem:options-parser.options.parse-opts-fn/test]
    #[test]
    fn tokenizes_and_populates_options() {
        let mut where_ = [
            opt("verbose", 'v', UOPT_NO_ARG),
            opt("grammar", 'g', UOPT_REQUIRES_ARG),
        ];
        // A dash flag, a long option, and a quoted argument for -g.
        let mut p = buf("--verbose -g 'my grammar.cg3'");
        parse_opts(&mut p, &mut where_);

        assert!(where_[0].does_occur, "--verbose parsed");
        assert!(where_[1].does_occur, "-g parsed");
        // The quoted token preserved its embedded space.
        assert_eq!(where_[1].value, "my grammar.cg3");
    }

    // Double-quoted tokens are handled the same way, and a bare (non-dash,
    // non-quoted) trailing token runs to the buffer end — exercising the
    // one-byte-past-terminator quirk against the required extra NUL. Here the
    // bare token is compacted by u_parseArgs (not consumed as an option value).
    // (parse_opts facet lives on the primary test above.)
    #[test]
    fn double_quotes_and_trailing_bare_token() {
        let mut where_ = [opt("grammar", 'g', UOPT_REQUIRES_ARG)];
        // -g with a double-quoted arg, then a trailing bare word at buffer end.
        let mut p = buf("-g \"a b\" leftover");
        parse_opts(&mut p, &mut where_);
        assert!(where_[0].does_occur);
        assert_eq!(where_[0].value, "a b");
        // The trailing bare token at the buffer edge did not panic (the extra
        // NUL guarded the one-past read).
    }

    // `parse_opts_env` reads an environment variable; when set, its value is
    // parsed into the option table (appending both the readable and the guard
    // NUL); when unset, the table is left untouched.
    // [spec:cg3:sem:options-parser.options.parse-opts-env-fn/test]
    #[test]
    fn env_var_set_and_unset() {
        // A process-unique var name so parallel tests don't collide.
        let var = "CG3_TEST_PARSE_OPTS_ENV";

        // Unset -> nothing happens.
        unsafe {
            std::env::remove_var(var);
        }
        let mut where_ = [opt("verbose", 'v', UOPT_NO_ARG)];
        parse_opts_env(var, &mut where_);
        assert!(!where_[0].does_occur, "unset env leaves options untouched");

        // Set -> value is tokenized and parsed.
        unsafe {
            std::env::set_var(var, "--verbose");
        }
        let mut where2 = [opt("verbose", 'v', UOPT_NO_ARG)];
        parse_opts_env(var, &mut where2);
        assert!(where2[0].does_occur, "set env value parsed into the table");

        unsafe {
            std::env::remove_var(var);
        }
    }
}
