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
//! Remaining NOTEd elision: `--stderr` creates the redirect file (same
//! truncation side effect as the C++) but diagnostics still go to process
//! stderr — the engine has no redirectable error sink.

use std::io::{Read, Write};

use crate::arg_parser::parse_args;
use crate::binary_grammar::BinaryGrammar;
use std::sync::Arc;

use crate::grammar::{Grammar, GrammarCore, Reindexed};
use crate::grammar_writer::GrammarWriter;
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::is_cg3b;
use crate::options::{
    Opt, grammar_options_default, grammar_options_override, options, options_default,
    options_override,
};
use crate::options_parser::{parse_opts, parse_opts_env};
use crate::profiler::Profiler;
use crate::tag_regex::{TagRegex, TagRegexError, compile_tag_regex};
use crate::textual_parser::TextualParser;

use super::{
    CG3_TOO_OLD, EXIT_FAILURE, EXIT_SUCCESS, divvun_copyright, divvun_version_line, emit, fail,
    merge_options, to_argv,
};

/// A `--nrules` / `--nrules-v` pattern that would not compile.
///
/// The message keeps the C++ layout (cause, then flag and pattern), and the
/// cause is reachable underneath it rather than only as text. Only the failure
/// `kind` is spliced into the message: `TagRegexError`'s own `Display` opens
/// with "cannot compile regex for tag", which a rule-name filter is not.
#[derive(Debug, thiserror::Error)]
#[error("Error: invalid regex ({}) in {flag} {pattern}", .source.kind)]
struct NrulesError {
    flag: &'static str,
    pattern: String,
    #[source]
    source: Box<TagRegexError>,
}

/// Compile the `--nrules` / `--nrules-v` pattern held in `options`, or `None`
/// when the flag was not given.
///
/// Through the tag-regex seam, not `regex::Regex::new`. The C++ compiled these
/// with the same ICU regex engine as every tag pattern, so a filter and a
/// grammar tag spelled identically meant identically. Compiling the filter with
/// a different engine reintroduces exactly the divergences the seam exists to
/// close, on a pattern the same person authored: no `\Q...\E`, ICU's `\Z`/`$`
/// misread as end-of-haystack, `[:script=Greek:]` silently read as a literal
/// character set, and possessive quantifiers taken as ordinary ones so the
/// filter selects MORE rules than asked for.
// [spec:cg3:req:tag-regex.single-seam+1]
fn nrules_pattern(
    options: &crate::options::OptionsTable,
    opt: Opt,
    flag: &'static str,
) -> Result<Option<TagRegex>, NrulesError> {
    if !options[opt as usize].does_occur {
        return Ok(None);
    }
    let pattern = &options[opt as usize].value;
    // Case-sensitive: the C++ compiles it with no flags.
    compile_tag_regex(pattern, false)
        .map(Some)
        .map_err(|source| NrulesError {
            flag,
            pattern: pattern.clone(),
            source,
        })
}

// [spec:cg3:def:main.main-fn+3]
// [spec:cg3:sem:main.main-fn+3]
// [spec:cg3:req:main.divvun-version-banner+2]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_run(args: &[String]) -> i32 {
    // clock_t main_timer = clock(); — timers dropped (verbose timing lines below
    // are ported without the actual durations).

    let status: i32 = 0;
    // srand(...) dropped (no rand() dependency in the ported paths).

    let prog = args.first().map(|s| s.as_str()).unwrap_or("vislcg3");

    // Owned option tables.
    let mut options = options();
    let mut options_default = options_default();
    let mut options_override = options_override();
    let mut grammar_options_default = grammar_options_default();
    let mut grammar_options_override = grammar_options_override();

    let mut argv = to_argv(args);
    let mut argc = parse_args(
        argv.len() as i32,
        &mut argv,
        Opt::NumOptions as i32,
        &mut options,
    );

    parse_opts_env("CG3_DEFAULT", &mut options_default);
    parse_opts_env("CG3_OVERRIDE", &mut options_override);
    merge_options(&mut options, &options_default, &options_override, None);

    let occ = |opts: &crate::options::OptionsTable, o: Opt| opts[o as usize].does_occur;

    // --debug: the C++ `debug_level` flag only ever gated verbose diagnostics, so
    // the port collapses it to "raise the tracing level to DEBUG" (no engine state).
    super::enable_debug_logging(occ(&options, Opt::Dodebug));

    // --min-binary-revision
    if occ(&options, Opt::VersionTooOld) {
        emit(std::io::stdout(), &format!("{CG3_TOO_OLD}\n"));
        return 0;
    }

    // --version / --help print the version line to stdout.
    if occ(&options, Opt::Version) || occ(&options, Opt::Help1) || occ(&options, Opt::Help2) {
        emit(std::io::stdout(), &divvun_version_line("Disambiguator"));
    }

    if argc < 0 {
        // argv[-argc] is the offending token.
        let bad = args.get((-argc) as usize).map(|s| s.as_str()).unwrap_or("");
        tracing::error!("{}: error in command line argument \"{}\"", prog, bad);
        return argc;
    }

    if occ(&options, Opt::Version) {
        emit(std::io::stdout(), &divvun_copyright());
        return EXIT_SUCCESS;
    }

    if !occ(&options, Opt::Grammar) && !occ(&options, Opt::Help1) && !occ(&options, Opt::Help2) {
        tracing::error!("Error: No grammar specified - cannot continue!");
        argc = -argc;
    }

    if argc < 0 || occ(&options, Opt::Help1) || occ(&options, Opt::Help2) {
        print_help(&options);
        return if argc < 0 { EXIT_FAILURE } else { EXIT_SUCCESS };
    }

    // --show-* / --dump-ast imply --grammar-only; --grammar-only implies --verbose;
    // --quiet unsets --verbose; --verbose 0 unsets it too.
    if occ(&options, Opt::ShowUnusedSets)
        || occ(&options, Opt::ShowSetHashes)
        || occ(&options, Opt::DumpAst)
    {
        options[Opt::GrammarOnly as usize].does_occur = true;
    }
    if occ(&options, Opt::GrammarOnly) && !occ(&options, Opt::Verbose) {
        options[Opt::Verbose as usize].does_occur = true;
    }
    if occ(&options, Opt::Quiet) {
        options[Opt::Verbose as usize].does_occur = false;
    }
    if occ(&options, Opt::Verbose)
        && !options[Opt::Verbose as usize].value.is_empty()
        && options[Opt::Verbose as usize].value == "0"
    {
        options[Opt::Verbose as usize].does_occur = false;
    }

    if occ(&options, Opt::CodepageGlobal)
        || occ(&options, Opt::CodepageInput)
        || occ(&options, Opt::CodepageOutput)
        || occ(&options, Opt::CodepageGrammar)
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
    let mut out_stream: Box<dyn Write> = if occ(&options, Opt::Stdout) {
        match std::fs::File::create(&options[Opt::Stdout as usize].value) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()), // dead ofstream — see NOTE.
        }
    } else {
        Box::new(std::io::stdout())
    };
    if occ(&options, Opt::Stderr) {
        // std::ofstream(options[STDERR].value) — created (same truncation side
        // effect as C++), but NOTE: the engine has no redirectable error
        // stream, so diagnostics still go to process stderr.
        let _ = std::fs::File::create(&options[Opt::Stderr as usize].value);
    }
    let stdin_file: Option<std::fs::File> = if occ(&options, Opt::Stdin) {
        let path = options[Opt::Stdin as usize].value.clone();
        // int serr = stat(path, &info); if (serr) { ... CG3Quit(1); } — stat
        // returns -1 on failure, so the message prints "error -1".
        if std::fs::metadata(&path).is_err() {
            tracing::error!("Error: Cannot stat {} due to error {}!", path, -1);
            return EXIT_FAILURE;
        }
        // Open failure past stat → dead ifstream (empty input) — see NOTE.
        std::fs::File::open(&path).ok()
    } else {
        None
    };

    let verbose = occ(&options, Opt::Verbose);

    // Read the grammar's first 4 bytes to detect binary vs text.
    let grammar_path = options[Opt::Grammar as usize].value.clone();
    let mut head = [0u8; 4];
    {
        let mut input = match std::fs::File::open(&grammar_path) {
            Ok(f) => f,
            Err(_) => {
                tracing::error!("Error: Error opening {} for reading!", grammar_path);
                return EXIT_FAILURE;
            }
        };
        if input.read_exact(&mut head).is_err() {
            tracing::error!("Error: Error reading first 4 bytes from grammar!");
            return EXIT_FAILURE;
        }
    }

    let is_binary = is_cg3b(head);
    if is_binary {
        if verbose {
            tracing::info!("Info: Binary grammar detected.");
        }
        if occ(&options, Opt::DumpAst) {
            tracing::error!("Error: --dump-ast is for textual grammars only!");
            return EXIT_FAILURE;
        }
        if occ(&options, Opt::Profiling) {
            tracing::error!("Error: --profile is for textual grammars only!");
            return EXIT_FAILURE;
        }
    }

    // --profile persists to a SQLite database, which only the `profiler` feature
    // links in; reject it early otherwise (recording would silently never write).
    #[cfg(not(feature = "profiler"))]
    if occ(&options, Opt::Profiling) {
        tracing::error!("Error: --profile requires building cg3 with the `profiler` feature.");
        return EXIT_FAILURE;
    }

    // Profiler for --profile (textual grammars only).
    let mut profiler: Option<Profiler> = if occ(&options, Opt::Profiling) {
        Some(Profiler::default())
    } else {
        None
    };

    // Parse the grammar into an owned Grammar (parser owns it; moved out after).
    let verbosity_level: u32 = if verbose {
        let v = &options[Opt::Verbose as usize].value;
        if !v.is_empty() {
            v.parse().unwrap_or(1)
        } else {
            1
        }
    } else {
        0
    };

    // --nrules / --nrules-v: C++ main.cpp wires both onto the IGrammarParser
    // base whichever parser it builds, so they compile once here and move into
    // the one that gets built.
    let nrules = match nrules_pattern(&options, Opt::Nrules, "--nrules") {
        Ok(re) => re,
        Err(e) => {
            tracing::error!("{e}");
            return EXIT_FAILURE;
        }
    };
    let nrules_inv = match nrules_pattern(&options, Opt::NrulesInv, "--nrules-v") {
        Ok(re) => re,
        Err(e) => {
            tracing::error!("{e}");
            return EXIT_FAILURE;
        }
    };

    // [spec:cg3:req:diagnostics.sidecar+1]
    // The parse's own buffers, kept only when `--grammar-bin` will need them for
    // the companion file it writes. Retaining a multi-megabyte grammar text for
    // the whole run of every other invocation is exactly what
    // `[spec:cg3:req:diagnostics.source-lazy]` forbids.
    let mut grammar_sources: Vec<crate::error::ParseSource> = Vec::new();

    let mut grammar: GrammarCore = if is_binary {
        let mut parser = BinaryGrammar::new(GrammarCore::default());
        if verbose {
            parser.set_verbosity(verbosity_level);
        }
        parser.set_compatible(occ(&options, Opt::Vislcgcompat));
        // BinaryGrammar_read.cpp applies the --nrules filters at rule read time.
        parser.nrules = nrules;
        parser.nrules_inv = nrules_inv;
        if let Err(e) = parser.parse_grammar_filename(&grammar_path) {
            return fail(&e);
        }
        let mut g = parser.grammar;
        g.verbosity_level = verbosity_level;
        g
    } else {
        let mut parser = TextualParser::new(GrammarCore::default(), occ(&options, Opt::DumpAst));
        if verbose {
            parser.set_verbosity(verbosity_level);
        }
        parser.set_compatible(occ(&options, Opt::Vislcgcompat));
        parser.nrules = nrules;
        parser.nrules_inv = nrules_inv;

        // C++: `parser->profiler = profiler.get();` — move the profiler into
        // the parser for the duration of the parse (taken back below).
        parser.profiler = profiler.take();
        let buffer = match std::fs::read(&grammar_path) {
            Ok(b) => b,
            Err(_) => {
                tracing::error!("Error: Error opening {} for reading!", grammar_path);
                return EXIT_FAILURE;
            }
        };
        // [spec:cg3:req:diagnostics.source-named]
        if let Err(e) = parser.parse_grammar_named(&buffer, &grammar_path) {
            return fail(&e);
        }
        profiler = parser.profiler.take();

        // --dump-ast prints the parse tree to the output stream.
        if occ(&options, Opt::DumpAst) {
            parser.print_ast(&mut out_stream);
        }
        // --profile: capture the grammar AST into the profiler string table.
        if let Some(p) = profiler.as_mut() {
            let mut buf: Vec<u8> = Vec::new();
            parser.print_ast(&mut buf);
            let sz = p.add_string(&String::from_utf8_lossy(&buf));
            p.grammar_ast = sz;
        }

        if occ(&options, Opt::GrammarBin) {
            grammar_sources = parser.sources();
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
    merge_options(
        &mut options,
        &grammar_options_default,
        &grammar_options_override,
        Some(&options_override),
    );

    // --prefix: override the mapping prefix (must match a binary grammar's).
    if occ(&options, Opt::MappingPrefix) {
        let mp = options[Opt::MappingPrefix as usize]
            .value
            .chars()
            .next()
            .unwrap_or('@');
        if grammar.is_binary && grammar.mapping_prefix != mp {
            tracing::error!(
                "Error: Mapping prefix must match the one used for compiling the binary grammar!"
            );
            return EXIT_FAILURE;
        }
        grammar.mapping_prefix = mp;
    }

    if verbose {
        tracing::info!("Reindexing grammar...");
    }
    match grammar.reindex(
        occ(&options, Opt::ShowUnusedSets),
        occ(&options, Opt::ShowTags),
    ) {
        // --show-tags: the dump is the whole job. The C++ exit(0)s inside
        // reindex; the exit code is decided here instead.
        Ok(Reindexed::DumpedTags) => return EXIT_SUCCESS,
        Ok(Reindexed::Done) => {}
        Err(e) => return fail(&e),
    }

    if verbose {
        tracing::info!(
            "Grammar has {} sections, {} templates, {} rules, {} sets, {} tags.",
            grammar.sections.len(),
            grammar.templates.len(),
            grammar.rule_by_number.capacity(),
            grammar.sets_list.capacity(),
            grammar.single_tags().size()
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

    if occ(&options, Opt::Profiling) && occ(&options, Opt::GrammarOnly) {
        tracing::error!("Error: Cannot gather profiling data with no input to run grammar on.");
        return EXIT_FAILURE;
    }

    // --- The applicator run (FormatConverter). Base members are reached through
    // the converter's public shared-base accessors (`base()`/`base_mut()`) — the
    // composition analogue of the C++ public inheritance. ---
    // C++ `FormatConverter::runGrammarOnText` writes `has_relations` onto the
    // live grammar, which main still holds when it serialises below; the port
    // keeps that on the run (`EngineConfig::stream_relations`), so carry it out
    // of the applicator for `--grammar-bin` to fold back in.
    let mut stream_relations = false;
    // Loaded: from here on the grammar is shared and nothing edits it, until
    // the writers take it back below.
    let grammar = Arc::new(grammar);
    if !occ(&options, Opt::GrammarOnly) {
        use crate::grammar_applicator::{GrammarApplicator, StreamFormatKind};
        let base = GrammarApplicator::new(Grammar::default());
        let mut applicator = match crate::format_converter::FormatConverter::new(base) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        applicator.base_mut().cfg.fmt_input = StreamFormatKind::Cg;
        if occ(&options, Opt::InCg) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Cg;
        } else if occ(&options, Opt::InNiceline) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Niceline;
        } else if occ(&options, Opt::InApertium) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Apertium;
        } else if occ(&options, Opt::InFst) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Fst;
        } else if occ(&options, Opt::InPlain) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Plain;
        } else if occ(&options, Opt::InJsonl) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Jsonl;
        } else if occ(&options, Opt::InBinary) {
            applicator.base_mut().cfg.fmt_input = StreamFormatKind::Binary;
        }

        // applicator.setGrammar(&grammar); — the C++ points the applicator at
        // the grammar main holds, and main goes on holding it. Here the
        // applicator gets a run over the shared grammar (replacing the ctor's
        // dummy conv grammar) and main keeps its own handle for the writers.
        //
        // One grammar, two holders, and this is the seam a host repeats N times:
        // every extra pipeline costs an overlay, not a grammar.
        applicator.base_mut().grammar = Grammar::from_core(Arc::clone(&grammar));
        if let Err(e) = applicator.base_mut().set_grammar() {
            return fail(&e);
        }
        if let Err(e) = applicator.base_mut().set_options(&options) {
            return fail(&e);
        }
        // [spec:cg3:req:diagnostics.runtime-input-named]
        applicator.base_mut().cfg.input_name = input_name(&options);

        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Cg;
        if occ(&options, Opt::OutApertium) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Apertium;
            applicator.base_mut().cfg.unicode_tags = true;
        } else if occ(&options, Opt::OutFst) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Fst;
        } else if occ(&options, Opt::OutNiceline) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Niceline;
        } else if occ(&options, Opt::OutPlain) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Plain;
        } else if occ(&options, Opt::OutJsonl) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Jsonl;
        } else if occ(&options, Opt::OutBinary) {
            applicator.base_mut().cfg.fmt_output = StreamFormatKind::Binary;
        }

        // C++: `applicator.profiler = profiler.get();` — move the profiler into
        // the engine for the run (taken back after, for the final write).
        if occ(&options, Opt::Profiling) {
            applicator.base_mut().diag.profiler = profiler.take();
        }

        // The ported driver needs `R: Read + Seek`; buffer the input stream
        // into a Cursor.
        let mut input_bytes = Vec::new();
        match stdin_file {
            Some(mut f) => {
                let _ = f.read_to_end(&mut input_bytes);
            }
            None => {
                let _ = std::io::stdin().read_to_end(&mut input_bytes);
            }
        }
        let mut cursor = std::io::Cursor::new(input_bytes);
        if let Err(e) = applicator.run_grammar_on_text(&mut cursor, &mut out_stream) {
            return fail(&e);
        }

        // Carry out what the run learned that main's copy cannot see, and take
        // back the profiler (for the final `Profiler::write`). The grammar does
        // not travel: main never gave it away. Dropping the applicator at the
        // end of this block releases its handle on the core, which is what lets
        // the writers below take it back.
        stream_relations = applicator.base().cfg.stream_relations;
        #[cfg(feature = "profiler")]
        if profiler.is_none() {
            profiler = applicator.base_mut().diag.profiler.take();
        }
    }

    if let Err(e) = write_grammars(&options, grammar, &grammar_sources, stream_relations) {
        return fail(&e);
    }

    // --profile: write the profiling database.
    #[cfg(feature = "profiler")]
    if let Some(p) = profiler.as_ref() {
        let _ = p.write(&options[Opt::Profiling as usize].value);
    }

    status
}

// [spec:cg3:req:diagnostics.runtime-input-named]
/// What a runtime diagnostic should call the input stream: the `--stdin` file
/// when one was given, else the name for a stream with no file behind it.
///
/// A free function rather than an inline branch so `main_run`, which is already
/// one long option-dispatch, does not grow another.
fn input_name(options: &crate::options::OptionsTable) -> String {
    let opt = &options[Opt::Stdin as usize];
    if opt.does_occur {
        return opt.value.clone();
    }
    crate::grammar_applicator::STDIN_SOURCE_NAME.to_string()
}

/// `--grammar-out` / `--grammar-bin`, after the run.
///
/// Both writers EDIT what they serialise, so they need the grammar back from
/// the run; the applicator, main's only other holder, is gone by now.
fn write_grammars(
    options: &crate::options::OptionsTable,
    grammar: Arc<GrammarCore>,
    sources: &[crate::error::ParseSource],
    stream_relations: bool,
) -> Result<(), crate::error::Cg3Error> {
    let occurs = |o: Opt| options[o as usize].does_occur;
    if !occurs(Opt::GrammarOut) && !occurs(Opt::GrammarBin) {
        return Ok(());
    }
    let mut grammar =
        Arc::try_unwrap(grammar).map_err(|_| crate::error::GrammarError::CoreShared)?;

    // --grammar-out: write the grammar in textual form. LIVE.
    if occurs(Opt::GrammarOut) {
        let path = &options[Opt::GrammarOut as usize].value;
        match std::fs::File::create(path) {
            Ok(mut gout) => {
                let mut writer = GrammarWriter::new(&grammar);
                writer.write_grammar(&mut grammar, &mut gout);
                let _ = gout.flush();
            }
            Err(_) => {
                tracing::error!("Could not write grammar to {}", path);
            }
        }
    }

    // --grammar-bin: write the grammar in binary form. LIVE.
    if occurs(Opt::GrammarBin) {
        let path = &options[Opt::GrammarBin as usize].value;
        write_grammar_bin(path, grammar, sources, stream_relations)?;
    }
    Ok(())
}

// [spec:cg3:req:diagnostics.sidecar+1]
/// `--grammar-bin`: write the grammar in binary form, and its sources beside it.
///
/// Takes the grammar by value, because the binary writer owns what it
/// serialises.
///
/// Serialised into memory first so the companion file stamps exactly the bytes
/// that reached the disk. A failure to create the output is logged and survived,
/// as it always was; a failure part-way through writing it is not, because what
/// is on disk then is a truncated grammar.
fn write_grammar_bin(
    path: &str,
    mut grammar: GrammarCore,
    sources: &[crate::error::ParseSource],
    stream_relations: bool,
) -> Result<(), crate::error::Cg3Error> {
    // The run's binary-stream `has_relations` (C++ stamps it straight onto the
    // grammar mid-run, and this writer sees it) — folded in here so the emitted
    // BINF_RELATIONS bit is what it always was.
    grammar.has_relations |= stream_relations;
    let mut blob: Vec<u8> = Vec::new();
    let mut writer = BinaryGrammar::new(grammar);
    writer.write_binary_grammar(&mut blob)?;
    let grammar = writer.grammar;

    let Ok(mut gout) = std::fs::File::create(path) else {
        tracing::error!("Could not write grammar to {}", path);
        return Ok(());
    };
    gout.write_all(&blob)
        .and_then(|()| gout.flush())
        .map_err(crate::error::RunError::Io)?;

    // Only for a grammar that came from text: a binary-to-binary pass has no
    // sources to describe, and an empty companion file says nothing more than
    // an absent one.
    if !sources.is_empty() {
        crate::grammar_sources::write_beside(std::path::Path::new(path), &blob, &grammar, sources);
    }
    Ok(())
}

/// The `--help` usage banner (C++ inlined in `main`). Emits to stdout.
// faithful port: the C++ `for (i=0; i<NUM_OPTIONS; ++i)` scans cover the whole
// table — its length IS the enum constant (`OptionsTable`).
fn print_help(options: &crate::options::OptionsTable) {
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
    for o in options.iter() {
        if !o.description.is_empty() {
            longest = longest.max(o.long_name.map_or(0, |s| s.len()));
        }
    }
    for o in options.iter() {
        if !o.description.is_empty() {
            out.push(' ');
            if o.short_name != '\0' {
                out.push_str(&format!("-{},", o.short_name));
            } else {
                out.push_str("   ");
            }
            let ln = o.long_name.unwrap_or("");
            out.push_str(&format!(" --{}", ln));
            let mut ldiff = longest - ln.len();
            while ldiff > 0 {
                out.push(' ');
                ldiff -= 1;
            }
            out.push_str(&format!("  {}\n", o.description));
        }
    }
    emit(std::io::stdout(), &out);
}
