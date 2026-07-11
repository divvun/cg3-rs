//! Port of `src/options_parser.hpp` (`parse_opts` / `parse_opts_env`) —
//! wave 4 (native-string) form.
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
//! ## Native-string tokenizer (wave 4)
//! The C++ `parse_opts(char* p, ...)` mutates a NUL-terminated buffer in place
//! (writing NULs to terminate tokens) and relies on callers appending an EXTRA
//! guard NUL for its one-byte-past-the-terminator read. The port tokenizes a
//! plain `&str` by slicing — the mutation, the sentinel NULs, and the guard-NUL
//! quirk all disappear; the produced argv is identical for every input the C++
//! accepted. (Tokens are converted to the `Vec<char>` form at the
//! [`u_parseArgs`] boundary, which is ICU-domain code-unit territory.)

use crate::icu_uoptions::u_parseArgs;
use crate::inlines::isspace;
use crate::options::UOption;
use crate::types::UChar;

// [spec:cg3:def:options-parser.options.parse-opts-fn]
// [spec:cg3:sem:options-parser.options.parse-opts-fn]
//
// Tokenization rules (exactly the C++ scanner's):
// * whitespace separates tokens;
// * a token starting `-` runs to the next whitespace;
// * `"`/`'` quote a token (the quote chars excluded; runs to the matching
//   quote or end of input — the C++ SKIPTO stopped on the NUL);
// * any other token runs to the next whitespace.
// The C++ in-place NUL writes + the one-past-terminator guard-NUL quirk are
// dissolved (see the module note); observable argv is unchanged.
pub fn parse_opts(p: &str, where_: &mut [UOption]) {
    let mut argv: Vec<Vec<UChar>> = vec![Vec::new()]; // 0th element is the program name
    let chars: Vec<char> = p.chars().collect();
    let mut pos = 0usize;
    let len = chars.len();
    while pos < len {
        while pos < len && isspace(chars[pos]) {
            pos += 1;
        }
        if pos >= len {
            break;
        }
        let c = chars[pos];
        if c == '"' || c == '\'' {
            pos += 1;
            let start = pos;
            while pos < len && chars[pos] != c {
                pos += 1;
            }
            argv.push(chars[start..pos].to_vec());
            pos += 1;
        } else {
            // `-`-prefixed and bare tokens both run to the next whitespace.
            let start = pos;
            while pos < len && !isspace(chars[pos]) {
                pos += 1;
            }
            argv.push(chars[start..pos].to_vec());
            pos += 1;
        }
    }
    let n_opts = where_.len() as i32;
    u_parseArgs(argv.len() as i32, &mut argv, n_opts, where_);
}

// [spec:cg3:def:options-parser.options.parse-opts-env-fn]
// [spec:cg3:sem:options-parser.options.parse-opts-env-fn]
//
// Reads env var `which`; if set, parses its value into `where_`. (The C++
// copied the value and appended the guard NUL the old scanner needed; the
// native-string scanner needs no terminators.)
pub fn parse_opts_env(which: &str, where_: &mut [UOption]) {
    if let Ok(env) = std::env::var(which) {
        parse_opts(&env, where_);
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

    // `parse_opts` tokenizes an embedded command line (splitting on whitespace,
    // honoring `"`/`'` quotes and `-` option tokens) into an argv and feeds it to
    // u_parseArgs. Drives the dash-token, quoted-token, and bare-token paths.
    // [spec:cg3:sem:options-parser.options.parse-opts-fn/test]
    #[test]
    fn tokenizes_and_populates_options() {
        let mut where_ = [
            opt("verbose", 'v', UOPT_NO_ARG),
            opt("grammar", 'g', UOPT_REQUIRES_ARG),
        ];
        // A dash flag, a long option, and a quoted argument for -g.
        parse_opts("--verbose -g 'my grammar.cg3'", &mut where_);

        assert!(where_[0].does_occur, "--verbose parsed");
        assert!(where_[1].does_occur, "-g parsed");
        // The quoted token preserved its embedded space.
        assert_eq!(where_[1].value, "my grammar.cg3");
    }

    // Double-quoted tokens are handled the same way, and a bare (non-dash,
    // non-quoted) trailing token runs to the end of input (the old
    // one-byte-past-terminator quirk site — now just a slice to the end). Here
    // the bare token is compacted by u_parseArgs (not consumed as an option
    // value).
    #[test]
    fn double_quotes_and_trailing_bare_token() {
        let mut where_ = [opt("grammar", 'g', UOPT_REQUIRES_ARG)];
        parse_opts("-g \"a b\" leftover", &mut where_);
        assert!(where_[0].does_occur);
        assert_eq!(where_[0].value, "a b");
    }

    // `parse_opts_env` reads an environment variable; when set, its value is
    // parsed into the option table; when unset, the table is left untouched.
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
