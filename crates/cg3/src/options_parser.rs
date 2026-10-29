//! Port of `src/options_parser.hpp` (`parse_opts`) in native-string form, and
//! the merge of one option table onto another. The header's
//! `parse_opts_env` reads the process environment, so it is the tools', not
//! the library's.
//!
//! Tokenizes a command line embedded in text (an env var, or a grammar
//! `CMDARGS` directive) into an `argv` vector and feeds it to `parse_args` to
//! populate an [`ArgOption`] table.
//!
//! ## Genericity (NOTE)
//! The C++ functions are templated over the vislcg3 and cg-conv tables. Both
//! hold the same entry type, so the port takes a plain `&mut [ArgOption]`
//! (any `[ArgOption; N]` coerces).
//!
//! ## Native-string tokenizer (wave 4)
//! The C++ `parse_opts(char* p, ...)` mutates a NUL-terminated buffer in place
//! (writing NULs to terminate tokens) and relies on callers appending an EXTRA
//! guard NUL for its one-byte-past-the-terminator read. The port tokenizes a
//! plain `&str` by slicing — the mutation, the sentinel NULs, and the guard-NUL
//! quirk all disappear; the produced argv is identical for every input the C++
//! accepted. (Tokens are converted to the `Vec<char>` form at the
//! [`parse_args`] boundary, which walks tokens one character at a time.)

use crate::arg_parser::parse_args;
use crate::inlines::isspace;
use crate::options::ArgOption;

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
pub fn parse_opts(p: &str, where_: &mut [ArgOption]) {
    let mut argv: Vec<Vec<char>> = vec![Vec::new()]; // 0th element is the program name
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
    parse_args(argv.len() as i32, &mut argv, n_opts, where_);
}

/// Merge one pair of option tables onto `options`: `defaults` fill only what is
/// still unset, `overrides` win outright.
///
/// vislcg3 and cg-proc run this twice — once for the `CG3_DEFAULT` /
/// `CG3_OVERRIDE` environment tables, once for the grammar's own `cmdargs`. The
/// second pass additionally declines to overwrite anything the environment
/// already forced, which is what `already_forced` names.
pub fn merge_options(
    options: &mut crate::options::OptionsTable,
    defaults: &crate::options::OptionsTable,
    overrides: &crate::options::OptionsTable,
    already_forced: Option<&crate::options::OptionsTable>,
) {
    for i in 0..crate::options::Opt::NumOptions as usize {
        if defaults[i].does_occur && !options[i].does_occur {
            options[i] = defaults[i].clone();
        }
        let forced = already_forced.is_some_and(|t| t[i].does_occur);
        if overrides[i].does_occur && !forced {
            options[i] = overrides[i].clone();
        }
    }
}

// [spec:cg3:req:embed.grammar-options]
/// Merge the options a grammar carries onto `options`, as vislcg3 and cg-proc
/// merge them with their command line: `cmdargs` (`CMDARGS`) fill only what is
/// still unset, `cmdargs_override` (`CMDARGS-OVERRIDE`) win, except over what
/// `already_forced` names.
pub fn merge_grammar_options(
    options: &mut crate::options::OptionsTable,
    cmdargs: &str,
    cmdargs_override: &str,
    already_forced: Option<&crate::options::OptionsTable>,
) {
    let mut defaults = crate::options::grammar_options_default();
    let mut overrides = crate::options::grammar_options_override();
    if !cmdargs.is_empty() {
        parse_opts(cmdargs, &mut defaults);
    }
    if !cmdargs_override.is_empty() {
        parse_opts(cmdargs_override, &mut overrides);
    }
    merge_options(options, &defaults, &overrides, already_forced);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::HasArg;

    fn opt(long: &'static str, short: char, has_arg: HasArg) -> ArgOption {
        ArgOption::hidden(long, short, has_arg)
    }

    // `parse_opts` tokenizes an embedded command line (splitting on whitespace,
    // honoring `"`/`'` quotes and `-` option tokens) into an argv and feeds it to
    // parse_args. Drives the dash-token, quoted-token, and bare-token paths.
    // [spec:cg3:sem:options-parser.options.parse-opts-fn/test]
    #[test]
    fn tokenizes_and_populates_options() {
        let mut where_ = [
            opt("verbose", 'v', HasArg::No),
            opt("grammar", 'g', HasArg::Required),
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
    // the bare token is compacted by parse_args (not consumed as an option
    // value).
    #[test]
    fn double_quotes_and_trailing_bare_token() {
        let mut where_ = [opt("grammar", 'g', HasArg::Required)];
        parse_opts("-g \"a b\" leftover", &mut where_);
        assert!(where_[0].does_occur);
        assert_eq!(where_[0].value, "a b");
    }

    // A grammar's CMDARGS reach a run through this merge, so its three cases are
    // the library's contract: a default fills only an unset option, an override
    // replaces a set one, and an option the first pass forced keeps its value.
    #[test]
    fn merge_defaults_fill_overrides_win_forced_stays() {
        use crate::options::{Opt, options};
        let set = |table: &mut crate::options::OptionsTable, o: Opt, value: &str| {
            table[o as usize].does_occur = true;
            table[o as usize].value = value.to_string();
        };

        let mut run = options();
        set(&mut run, Opt::NumWindows, "3");
        let mut defaults = options();
        set(&mut defaults, Opt::NumWindows, "9");
        set(&mut defaults, Opt::SoftLimit, "100");
        let mut overrides = options();
        set(&mut overrides, Opt::HardLimit, "200");
        merge_options(&mut run, &defaults, &overrides, None);
        assert_eq!(run[Opt::NumWindows as usize].value, "3");
        assert_eq!(run[Opt::SoftLimit as usize].value, "100");
        assert_eq!(run[Opt::HardLimit as usize].value, "200");

        let mut forced = options();
        set(&mut forced, Opt::HardLimit, "200");
        let mut grammar_overrides = options();
        set(&mut grammar_overrides, Opt::HardLimit, "1");
        set(&mut grammar_overrides, Opt::SoftLimit, "7");
        merge_options(&mut run, &options(), &grammar_overrides, Some(&forced));
        assert_eq!(run[Opt::HardLimit as usize].value, "200");
        assert_eq!(run[Opt::SoftLimit as usize].value, "7");
    }
}
