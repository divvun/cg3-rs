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
    //
    // The "set" branch delegates verbatim to `parse_opts` (verified by
    // `tokenizes_and_populates_options` / `double_quotes_and_trailing_bare_token`
    // above), so this exercises the wrapper's env plumbing without mutating the
    // process environment (which `std::env::set_var`/`remove_var` require an
    // `unsafe` block for under Edition 2024, due to the getenv/setenv data race).
    // We read whatever value the runner already has for a couple of well-known
    // vars and confirm the forward-vs-untouched contract holds for both a
    // present var and a guaranteed-absent one.
    // [spec:cg3:sem:options-parser.options.parse-opts-env-fn/test]
    #[test]
    fn env_var_set_and_unset() {
        // A name essentially never present in the environment -> `env::var`
        // yields `Err`, so `parse_opts_env` must leave the table untouched.
        let absent = "CG3_TEST_PARSE_OPTS_ENV_DEFINITELY_UNSET_9f3a";
        assert!(
            std::env::var(absent).is_err(),
            "test precondition: {absent} must be unset"
        );
        let mut where_ = [opt("verbose", 'v', UOPT_NO_ARG)];
        parse_opts_env(absent, &mut where_);
        assert!(!where_[0].does_occur, "unset env leaves options untouched");

        // Present var: `PATH` is set in every runner environment. `parse_opts_env`
        // must read it and forward its value to `parse_opts` (which tokenizes it);
        // since `PATH` contains no recognized options, the table stays untouched,
        // but the delegation is what we assert did not panic / mis-handle.
        if let Ok(path) = std::env::var("PATH") {
            let mut where2 = [opt("verbose", 'v', UOPT_NO_ARG)];
            parse_opts_env("PATH", &mut where2);
            // Forwarding is equivalent to calling parse_opts on the raw value.
            let mut where3 = [opt("verbose", 'v', UOPT_NO_ARG)];
            parse_opts(&path, &mut where3);
            assert_eq!(
                where2[0].does_occur, where3[0].does_occur,
                "parse_opts_env forwards the env value to parse_opts"
            );
        }
    }
}
