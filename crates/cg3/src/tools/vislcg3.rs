//! Port of `src/main.cpp` — the `vislcg3` disambiguator (main entry point).
//!
//! Parses the full vislcg3 option set, loads a grammar (binary or text),
//! reindexes, optionally runs it over stdin→stdout via a
//! [`crate::format_converter::FormatConverter`] (configured through its public
//! shared-base accessors — the composition analogue of the C++ public
//! inheritance), and optionally writes the grammar back out in textual
//! ([`crate::grammar_writer::GrammarWriter`]) and/or binary
//! ([`crate::binary_grammar::BinaryGrammar`]) form.
//!
//! Remaining NOTEd elisions: the `--nrules` / `--nrules-inv` regex filters
//! (parser internals not surfaced publicly) and the engine's `profiler` field
//! being an `Option<()>` placeholder (the `--profile` database `write` is live,
//! but no per-rule data is gathered by the run).

use std::io::{Read, Write};

use crate::binary_grammar::BinaryGrammar;
use crate::grammar::Grammar;
use crate::grammar_writer::GrammarWriter;
use crate::icu_uoptions::u_parseArgs;
use crate::inlines::{cg3_quit, is_cg3b};
use crate::options::{
    OPTIONS, grammar_options_default, grammar_options_override, options, options_default,
    options_override,
};
use crate::options_parser::{parse_opts, parse_opts_env};
use crate::profiler::Profiler;
use crate::textual_parser::TextualParser;

use super::{
    CG3_COPYRIGHT_STRING, CG3_REVISION, CG3_TOO_OLD, CG3_VERSION_MAJOR, CG3_VERSION_MINOR,
    CG3_VERSION_PATCH, U_ILLEGAL_ARGUMENT_ERROR, U_ZERO_ERROR, to_uargv,
};

// [spec:cg3:def:main.main-fn]
// [spec:cg3:sem:main.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_run(args: &[String]) -> i32 {
    // clock_t main_timer = clock(); — timers dropped (verbose timing lines below
    // are ported without the actual durations).

    // UErrorCode status = U_ZERO_ERROR;
    let status: i32 = 0;
    // srand(...) dropped (no rand() dependency in the ported paths).

    let prog = args.first().map(|s| s.as_str()).unwrap_or("vislcg3");

    // Owned option tables.
    let mut options = options();
    let mut options_default = options_default();
    let mut options_override = options_override();
    let mut grammar_options_default = grammar_options_default();
    let mut grammar_options_override = grammar_options_override();

    // argc = u_parseArgs(argc, argv, options.size(), options.data());
    let mut argv = to_uargv(args);
    let mut argc = u_parseArgs(
        argv.len() as i32,
        &mut argv,
        OPTIONS::NUM_OPTIONS as i32,
        &mut options,
    );

    parse_opts_env("CG3_DEFAULT", &mut options_default);
    parse_opts_env("CG3_OVERRIDE", &mut options_override);
    for i in 0..OPTIONS::NUM_OPTIONS as usize {
        if options_default[i].does_occur && !options[i].does_occur {
            options[i] = options_default[i].clone();
        }
        if options_override[i].does_occur {
            options[i] = options_override[i].clone();
        }
    }

    let occ = |opts: &crate::options::options_t, o: OPTIONS| opts[o as usize].does_occur;

    // --min-binary-revision
    if occ(&options, OPTIONS::VERSION_TOO_OLD) {
        println!("{}", CG3_TOO_OLD);
        return 0;
    }

    // --version / --help print the version line to stdout.
    if occ(&options, OPTIONS::VERSION)
        || occ(&options, OPTIONS::HELP1)
        || occ(&options, OPTIONS::HELP2)
    {
        println!(
            "VISL CG-3 Disambiguator version {}.{}.{}.{}",
            CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH, CG3_REVISION
        );
    }

    if argc < 0 {
        // argv[-argc] is the offending token.
        let bad = args.get((-argc) as usize).map(|s| s.as_str()).unwrap_or("");
        tracing::error!("{}: error in command line argument \"{}\"", prog, bad);
        return argc;
    }

    if occ(&options, OPTIONS::VERSION) {
        println!("{}", CG3_COPYRIGHT_STRING);
        return U_ZERO_ERROR;
    }

    if !occ(&options, OPTIONS::GRAMMAR)
        && !occ(&options, OPTIONS::HELP1)
        && !occ(&options, OPTIONS::HELP2)
    {
        tracing::error!("Error: No grammar specified - cannot continue!");
        argc = -argc;
    }

    if argc < 0 || occ(&options, OPTIONS::HELP1) || occ(&options, OPTIONS::HELP2) {
        print_help(&options);
        return if argc < 0 {
            U_ILLEGAL_ARGUMENT_ERROR
        } else {
            U_ZERO_ERROR
        };
    }

    // --show-* / --dump-ast imply --grammar-only; --grammar-only implies --verbose;
    // --quiet unsets --verbose; --verbose 0 unsets it too.
    if occ(&options, OPTIONS::SHOW_UNUSED_SETS)
        || occ(&options, OPTIONS::SHOW_SET_HASHES)
        || occ(&options, OPTIONS::DUMP_AST)
    {
        options[OPTIONS::GRAMMAR_ONLY as usize].does_occur = true;
    }
    if occ(&options, OPTIONS::GRAMMAR_ONLY) && !occ(&options, OPTIONS::VERBOSE) {
        options[OPTIONS::VERBOSE as usize].does_occur = true;
    }
    if occ(&options, OPTIONS::QUIET) {
        options[OPTIONS::VERBOSE as usize].does_occur = false;
    }
    if occ(&options, OPTIONS::VERBOSE)
        && !options[OPTIONS::VERBOSE as usize].value.is_empty()
        && options[OPTIONS::VERBOSE as usize].value == "0"
    {
        options[OPTIONS::VERBOSE as usize].does_occur = false;
    }

    // ICU init / codepage / locale dropped (UTF-8 port).
    if occ(&options, OPTIONS::CODEPAGE_GLOBAL)
        || occ(&options, OPTIONS::CODEPAGE_INPUT)
        || occ(&options, OPTIONS::CODEPAGE_OUTPUT)
        || occ(&options, OPTIONS::CODEPAGE_GRAMMAR)
    {
        tracing::warn!(
            "Warning: The -C and --codepage-* option are deprecated and now default to UTF-8"
        );
    }

    // --stdout / --stderr / --stdin file redirection (C++ opens these up-front).
    // NOTE: the C++ failure checks are `!stream || stream->bad()` — an
    // ofstream/ifstream that FAILS to open sets failbit, not badbit, so `bad()`
    // is false and those checks never fire; the C++ proceeds with a dead stream
    // (output silently discarded / input reads as empty). Mirrored here with
    // sink()/empty-input fallbacks. The `--stdin` stat() failure DOES exit.
    let mut ux_stdout: Box<dyn Write> = if occ(&options, OPTIONS::STDOUT) {
        match std::fs::File::create(&options[OPTIONS::STDOUT as usize].value) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()), // dead ofstream — see NOTE.
        }
    } else {
        Box::new(std::io::stdout())
    };
    if occ(&options, OPTIONS::STDERR) {
        // std::ofstream(options[STDERR].value) — created (same truncation side
        // effect as C++), but NOTE: the engine's `ux_stderr` is an elided
        // placeholder in this port, so diagnostics still go to process stderr.
        let _ = std::fs::File::create(&options[OPTIONS::STDERR as usize].value);
    }
    let ux_stdin_file: Option<std::fs::File> = if occ(&options, OPTIONS::STDIN) {
        let path = options[OPTIONS::STDIN as usize].value.clone();
        // int serr = stat(path, &info); if (serr) { ... CG3Quit(1); } — stat
        // returns -1 on failure, so the message prints "error -1".
        if std::fs::metadata(&path).is_err() {
            tracing::error!("Error: Cannot stat {} due to error {}!", path, -1);
            cg3_quit(1, None, 0);
        }
        // Open failure past stat → dead ifstream (empty input) — see NOTE.
        std::fs::File::open(&path).ok()
    } else {
        None
    };

    let verbose = occ(&options, OPTIONS::VERBOSE);

    // Read the grammar's first 4 bytes to detect binary vs text.
    let grammar_path = options[OPTIONS::GRAMMAR as usize].value.clone();
    let mut head = [0u8; 4];
    {
        let mut input = match std::fs::File::open(&grammar_path) {
            Ok(f) => f,
            Err(_) => {
                tracing::error!("Error: Error opening {} for reading!", grammar_path);
                cg3_quit(1, None, 0);
            }
        };
        if input.read_exact(&mut head).is_err() {
            tracing::error!("Error: Error reading first 4 bytes from grammar!");
            cg3_quit(1, None, 0);
        }
    }

    let is_binary = is_cg3b(head);
    if is_binary {
        if verbose {
            tracing::info!("Info: Binary grammar detected.");
        }
        if occ(&options, OPTIONS::DUMP_AST) {
            tracing::error!("Error: --dump-ast is for textual grammars only!");
            cg3_quit(1, None, 0);
        }
        if occ(&options, OPTIONS::PROFILING) {
            tracing::error!("Error: --profile is for textual grammars only!");
            cg3_quit(1, None, 0);
        }
    }

    // Profiler for --profile (textual grammars only).
    let mut profiler: Option<Profiler> = if occ(&options, OPTIONS::PROFILING) {
        Some(Profiler::default())
    } else {
        None
    };

    // Parse the grammar into an owned Grammar (parser owns it; moved out after).
    let verbosity_level: u32 = if verbose {
        let v = &options[OPTIONS::VERBOSE as usize].value;
        if !v.is_empty() {
            v.parse().unwrap_or(1)
        } else {
            1
        }
    } else {
        0
    };

    let mut grammar: Grammar = if is_binary {
        let mut parser = BinaryGrammar::binary_grammar(Grammar::default());
        if verbose {
            parser.set_verbosity(verbosity_level);
        }
        parser.set_compatible(occ(&options, OPTIONS::VISLCGCOMPAT));
        // C++ main.cpp wires --nrules/--nrules-v on the IGrammarParser base for
        // both parsers; BinaryGrammar_read.cpp applies them at rule read time.
        if occ(&options, OPTIONS::NRULES) {
            let pat = &options[OPTIONS::NRULES as usize].value;
            match regex::Regex::new(pat) {
                Ok(re) => parser.nrules = Some(re),
                Err(e) => {
                    tracing::error!(
                        "Error: uregex_open returned {} trying to parse --nrules {}",
                        e,
                        pat
                    );
                    cg3_quit(1, None, 0);
                }
            }
        }
        if occ(&options, OPTIONS::NRULES_INV) {
            let pat = &options[OPTIONS::NRULES_INV as usize].value;
            match regex::Regex::new(pat) {
                Ok(re) => parser.nrules_inv = Some(re),
                Err(e) => {
                    tracing::error!(
                        "Error: uregex_open returned {} trying to parse --nrules-v {}",
                        e,
                        pat
                    );
                    cg3_quit(1, None, 0);
                }
            }
        }
        if parser.parse_grammar_filename(&grammar_path) != 0 {
            tracing::error!("Error: Grammar could not be parsed - exiting!");
            cg3_quit(1, None, 0);
        }
        let mut g = parser.grammar;
        g.verbosity_level = verbosity_level;
        g
    } else {
        let mut parser = TextualParser::new(Grammar::default(), occ(&options, OPTIONS::DUMP_AST));
        if verbose {
            parser.set_verbosity(verbosity_level);
        }
        parser.set_compatible(occ(&options, OPTIONS::VISLCGCOMPAT));

        // if (options[NRULES].doesOccur) { parser->nrules = uregex_open(...); }
        // (ICU converter dance dropped in the UTF-8 port; compile failure exits
        // like the C++ status check.)
        if occ(&options, OPTIONS::NRULES) {
            let pat = &options[OPTIONS::NRULES as usize].value;
            match regex::Regex::new(pat) {
                Ok(re) => parser.nrules = Some(re),
                Err(e) => {
                    tracing::error!(
                        "Error: uregex_open returned {} trying to parse --nrules {}",
                        e,
                        pat
                    );
                    cg3_quit(1, None, 0);
                }
            }
        }
        if occ(&options, OPTIONS::NRULES_INV) {
            let pat = &options[OPTIONS::NRULES_INV as usize].value;
            match regex::Regex::new(pat) {
                Ok(re) => parser.nrules_inv = Some(re),
                Err(e) => {
                    tracing::error!(
                        "Error: uregex_open returned {} trying to parse --nrules-v {}",
                        e,
                        pat
                    );
                    cg3_quit(1, None, 0);
                }
            }
        }

        // C++: `parser->profiler = profiler.get();` — move the profiler into
        // the parser for the duration of the parse (taken back below).
        parser.profiler = profiler.take();
        let buffer = match std::fs::read(&grammar_path) {
            Ok(b) => b,
            Err(_) => {
                tracing::error!("Error: Error opening {} for reading!", grammar_path);
                cg3_quit(1, None, 0);
            }
        };
        if parser.parse_grammar_utf8(&buffer) != 0 {
            tracing::error!("Error: Grammar could not be parsed - exiting!");
            cg3_quit(1, None, 0);
        }
        profiler = parser.profiler.take();

        // --dump-ast prints the parse tree to *ux_stdout.
        if occ(&options, OPTIONS::DUMP_AST) {
            parser.print_ast(&mut ux_stdout);
        }
        // --profile: capture the grammar AST into the profiler string table.
        if let Some(p) = profiler.as_mut() {
            let mut buf: Vec<u8> = Vec::new();
            parser.print_ast(&mut buf);
            let sz = p.add_string(&String::from_utf8_lossy(&buf));
            p.grammar_ast = sz;
        }

        let mut g = parser.grammar;
        g.verbosity_level = verbosity_level;
        g
    };

    // Grammar cmdargs → parse_opts into grammar_options_{default,override}, merge.
    if !grammar.cmdargs.is_empty() {
        parse_opts(&grammar.cmdargs, &mut grammar_options_default);
    }
    if !grammar.cmdargs_override.is_empty() {
        parse_opts(&grammar.cmdargs_override, &mut grammar_options_override);
    }
    for i in 0..OPTIONS::NUM_OPTIONS as usize {
        if grammar_options_default[i].does_occur && !options[i].does_occur {
            options[i] = grammar_options_default[i].clone();
        }
        if grammar_options_override[i].does_occur && !options_override[i].does_occur {
            options[i] = grammar_options_override[i].clone();
        }
    }

    // --prefix: override the mapping prefix (must match a binary grammar's).
    if occ(&options, OPTIONS::MAPPING_PREFIX) {
        let mp = options[OPTIONS::MAPPING_PREFIX as usize]
            .value
            .chars()
            .next()
            .unwrap_or('@');
        if grammar.is_binary && grammar.mapping_prefix != mp {
            tracing::error!(
                "Error: Mapping prefix must match the one used for compiling the binary grammar!"
            );
            cg3_quit(1, None, 0);
        }
        grammar.mapping_prefix = mp;
    }

    if verbose {
        tracing::info!("Reindexing grammar...");
    }
    grammar.reindex(
        occ(&options, OPTIONS::SHOW_UNUSED_SETS),
        occ(&options, OPTIONS::SHOW_TAGS),
    );

    if verbose {
        tracing::info!(
            "Grammar has {} sections, {} templates, {} rules, {} sets, {} tags.",
            grammar.sections.len(),
            grammar.templates.len(),
            grammar.rule_by_number.capacity(),
            grammar.sets_list.capacity(),
            grammar.single_tags.size()
        );
        if let Some(rules_any) = grammar.rules_any.as_ref() {
            tracing::info!("{} rules cannot be skipped by index.", rules_any.size());
        }
        if grammar.has_dep {
            tracing::info!("Grammar has dependency rules.");
        }
        if grammar.has_relations {
            tracing::info!("Grammar has relation rules.");
        }
    }

    if occ(&options, OPTIONS::PROFILING) && occ(&options, OPTIONS::GRAMMAR_ONLY) {
        tracing::error!("Error: Cannot gather profiling data with no input to run grammar on.");
        cg3_quit(1, None, 0);
    }

    // --- The applicator run (FormatConverter). Base members are reached through
    // the converter's public shared-base accessors (`base()`/`base_mut()`) — the
    // composition analogue of the C++ public inheritance. ---
    if !occ(&options, OPTIONS::GRAMMAR_ONLY) {
        use crate::grammar_applicator::{GrammarApplicator, cg3_sformat};
        let base = GrammarApplicator::new(Grammar::default());
        let mut applicator = crate::format_converter::FormatConverter::new(base);
        applicator.base_mut().fmt_input = cg3_sformat::CG3SF_CG;
        if occ(&options, OPTIONS::IN_CG) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_CG;
        } else if occ(&options, OPTIONS::IN_NICELINE) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_NICELINE;
        } else if occ(&options, OPTIONS::IN_APERTIUM) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_APERTIUM;
        } else if occ(&options, OPTIONS::IN_FST) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_FST;
        } else if occ(&options, OPTIONS::IN_PLAIN) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_PLAIN;
        } else if occ(&options, OPTIONS::IN_JSONL) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_JSONL;
        } else if occ(&options, OPTIONS::IN_BINARY) {
            applicator.base_mut().fmt_input = cg3_sformat::CG3SF_BINARY;
        }

        // applicator.setGrammar(&grammar); — the ported base OWNS its grammar,
        // so "point the applicator at the externally-held grammar" becomes:
        // move the parsed grammar in (replacing the ctor's dummy conv grammar),
        // seed begin/end/subst tags, and move it back out after the run for the
        // --grammar-out / --grammar-bin writers below.
        applicator.base_mut().grammar = grammar;
        applicator.base_mut().set_grammar();
        // applicator.setOptions(conv); (UConverter dropped in the UTF-8 port).
        applicator.base_mut().set_options(&options);

        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_CG;
        if occ(&options, OPTIONS::OUT_APERTIUM) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_APERTIUM;
            applicator.base_mut().unicode_tags = true;
        } else if occ(&options, OPTIONS::OUT_FST) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_FST;
        } else if occ(&options, OPTIONS::OUT_NICELINE) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_NICELINE;
        } else if occ(&options, OPTIONS::OUT_PLAIN) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_PLAIN;
        } else if occ(&options, OPTIONS::OUT_JSONL) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_JSONL;
        } else if occ(&options, OPTIONS::OUT_BINARY) {
            applicator.base_mut().fmt_output = cg3_sformat::CG3SF_BINARY;
        }

        // C++: `applicator.profiler = profiler.get();` — move the profiler into
        // the engine for the run (taken back after, for the final write).
        if occ(&options, OPTIONS::PROFILING) {
            applicator.base_mut().profiler = profiler.take();
        }

        // applicator.runGrammarOnText(*ux_stdin, *ux_stdout); — the ported
        // driver needs `R: Read + Seek`; buffer the input stream into a Cursor.
        let mut input_bytes = Vec::new();
        match ux_stdin_file {
            Some(mut f) => {
                let _ = f.read_to_end(&mut input_bytes);
            }
            None => {
                let _ = std::io::stdin().read_to_end(&mut input_bytes);
            }
        }
        let mut cursor = std::io::Cursor::new(input_bytes);
        applicator.run_grammar_on_text(&mut cursor, &mut ux_stdout);

        // Move the grammar back out (C++ `grammar` lives in main throughout),
        // and the profiler (for the final `Profiler::write`).
        grammar = std::mem::replace(&mut applicator.base_mut().grammar, Grammar::default());
        if profiler.is_none() {
            profiler = applicator.base_mut().profiler.take();
        }
    }

    // --grammar-out: write the grammar in textual form. LIVE.
    if occ(&options, OPTIONS::GRAMMAR_OUT) {
        let path = &options[OPTIONS::GRAMMAR_OUT as usize].value;
        match std::fs::File::create(path) {
            Ok(mut gout) => {
                let mut writer = GrammarWriter::grammar_writer(&grammar);
                writer.write_grammar(&mut grammar, &mut gout);
                let _ = gout.flush();
            }
            Err(_) => {
                tracing::error!("Could not write grammar to {}", path);
            }
        }
    }

    // --grammar-bin: write the grammar in binary form. LIVE.
    if occ(&options, OPTIONS::GRAMMAR_BIN) {
        let path = options[OPTIONS::GRAMMAR_BIN as usize].value.clone();
        match std::fs::File::create(&path) {
            Ok(mut gout) => {
                let mut writer = BinaryGrammar::binary_grammar(grammar);
                writer.write_binary_grammar(&mut gout);
                let _ = gout.flush();
                grammar = writer.grammar;
            }
            Err(_) => {
                tracing::error!("Could not write grammar to {}", path);
            }
        }
    }
    let _ = &grammar;

    // --profile: write the profiling database.
    if let Some(p) = profiler.as_ref() {
        let _ = p.write(&options[OPTIONS::PROFILING as usize].value);
    }

    // u_cleanup dropped.
    status
}

/// The `--help` usage banner (C++ inlined in `main`). Emits to stdout.
fn print_help(options: &crate::options::options_t) {
    let mut out = String::new();
    out.push_str("Usage: vislcg3 [OPTIONS]\n");
    out.push('\n');
    out.push_str("Environment variable:\n");
    out.push_str(" CG3_DEFAULT: Sets default cmdline options, which the actual passed options will override.\n");
    out.push_str(
        " CG3_OVERRIDE: Sets forced cmdline options, which will override any passed option.\n",
    );
    out.push('\n');
    out.push_str("Options:\n");

    let mut longest = 0usize;
    for i in 0..OPTIONS::NUM_OPTIONS as usize {
        if !options[i].description.is_empty() {
            longest = longest.max(options[i].long_name.map_or(0, |s| s.len()));
        }
    }
    for i in 0..OPTIONS::NUM_OPTIONS as usize {
        if !options[i].description.is_empty() {
            out.push(' ');
            if options[i].short_name != '\0' {
                out.push_str(&format!("-{},", options[i].short_name));
            } else {
                out.push_str("   ");
            }
            let ln = options[i].long_name.unwrap_or("");
            out.push_str(&format!(" --{}", ln));
            let mut ldiff = longest - ln.len();
            while ldiff > 0 {
                out.push(' ');
                ldiff -= 1;
            }
            out.push_str(&format!("  {}\n", options[i].description));
        }
    }
    print!("{}", out);
}
