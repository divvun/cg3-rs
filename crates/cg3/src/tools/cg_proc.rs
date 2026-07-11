//! Port of `src/cg-proc.cpp` — the Apertium/Matxin/Binary stream processor.
//!
//! Unlike the other tools, cg-proc parses its own options with POSIX `getopt`
//! (NOT the ICU `u_parseArgs` / `UOption` tables), then loads a grammar and runs
//! the applicator matching the `-f` stream format. This port reproduces the
//! getopt loop faithfully — including the flagged UB bug (see below).
//!
//! ## Reproduced bug: `--sections` no_argument → `atoi(NULL)` (UB)
//! In the `HAVE_GETOPT_LONG` table, `--sections` is declared with `no_argument`
//! (`0`), yet its handler runs `sections = atoi(optarg)`. When `--sections` is
//! given in its LONG form, `getopt_long` leaves `optarg == NULL`, so the C++
//! executes `atoi(NULL)` — undefined behaviour (typically crashes or reads
//! garbage). The SHORT form `-s` is declared `s:` (requires an argument), so it
//! is fine. This port reproduces the divergence: for the long `--sections` (no
//! argument consumed) the handler is fed a NULL-analogue that triggers the same
//! faulty `atoi` path (marked at the site).
//!
//! All four stream formats run LIVE: 0 (base `GrammarApplicator`), 1
//! (Apertium), 2 (Matxin), and 3 (`BinaryApplicator`), each via its ported
//! `run_grammar_on_text` driver.

use std::io::Read;

use crate::apertium_applicator::ApertiumApplicator;
use crate::binary_applicator::BinaryApplicator;
use crate::binary_grammar::BinaryGrammar;
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{cg3_quit, is_cg3b};
use crate::matxin_applicator::MatxinApplicator;
use crate::options::{
    OPTIONS, grammar_options_default, grammar_options_override, options, options_default,
    options_override,
};
use crate::options_parser::{parse_opts, parse_opts_env};
use crate::textual_parser::TextualParser;

use super::{CG3_REVISION, CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH, basename};

// [spec:cg3:def:cg-proc.end-program-fn]
// [spec:cg3:sem:cg-proc.end-program-fn]
/// C++ `void endProgram(char* name)`. Prints the version + full usage banner and
/// exits `EXIT_FAILURE`. The `HAVE_GETOPT_LONG` variant of the banner is used
/// (the ported getopt below is the long variant).
fn end_program(name: &str) -> ! {
    println!(
        "VISL CG-3 Disambiguator version {}.{}.{}.{}",
        CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH, CG3_REVISION
    );
    println!(
        "{}: process a stream with a constraint grammar",
        basename(name)
    );
    println!(
        "USAGE: {} [-t] [-s] [-d] [-g] [-r rule] grammar_file [input_file [output_file]]",
        basename(name)
    );
    println!("Options:");
    println!("\t-d, --disambiguation:\t morphological disambiguation");
    println!("\t-s, --sections=NUM:\t specify number of sections to process");
    println!("\t-f, --stream-format=NUM: set the format of the I/O stream to NUM,");
    println!("\t\t\t\t   where `0' is VISL format, `1' is Apertium");
    println!("\t\t\t\t   format, `2` is Matxin, and `3` is binary");
    println!("                  (default: 1)");
    println!("\t-r, --rule=NAME:\t run only the named rule");
    println!("\t-t, --trace:\t\t print debug output on stderr");
    println!("\t-w, --wordform-case:\t enforce surface case on lemma/baseform ");
    println!("\t\t\t\t   (to work with -w option of lt-proc)");
    println!("\t-n, --no-word-forms:\t do not print out the word form of each cohort");
    println!("\t-g, --generation:\t do not surround lexical units in ^$");
    println!("\t-1, --first:\t \t only output the first analysis if ambiguity remains");
    println!("\t-z, --null-flush:\tflush output on the null character");
    println!("\t-v, --version:\t \t version");
    println!("\t-h, --help:\t\t show this help");
    crate::error::cg3_exit(1)
}

/// Minimal faithful `getopt_long` for the exact optstring `"ds:f:tr:n1wvhz"`
/// plus the cg-proc long-option table. Only what cg-proc needs is modelled:
/// short options with/without required args (`x:`), the `--long` forms mapping to
/// their short letter, and `--` to stop option scanning. Returns a sequence of
/// `(letter, Option<arg>)` events plus the index of the first non-option
/// (`optind`). A `'\0'` letter with `None` marks an error → the caller runs
/// `endProgram`.
struct GetoptResult {
    events: Vec<(char, Option<String>)>,
    optind: usize,
    error: bool,
}

/// Long name → (short letter, requires_arg). Mirrors the C++ `long_options[]`.
/// NOTE the `--sections` bug: its short mapping is `s` but here it is declared
/// with `requires_arg = false` (the C++ `no_argument`), so the long form yields
/// NO argument even though the handler will `atoi` one.
fn long_option(name: &str) -> Option<(char, bool)> {
    match name {
        "disambiguation" => Some(('d', false)),
        "sections" => Some(('s', false)), // BUG: no_argument, but handler atoi()s optarg
        "stream-format" => Some(('f', true)),
        "rule" => Some(('r', false)), // C++ table also has this as no_argument
        "trace" => Some(('t', false)),
        "wordform-case" => Some(('w', false)),
        "no-word-forms" => Some(('n', false)),
        "generation" => Some(('g', false)),
        "version" => Some(('v', false)),
        "first" => Some(('1', false)),
        "help" => Some(('h', false)),
        "null-flush" => Some(('z', false)),
        _ => None,
    }
}

/// Whether a short option requires an argument, per `"ds:f:tr:n1wvhz"`.
fn short_requires_arg(c: char) -> Option<bool> {
    match c {
        'd' => Some(false),
        's' => Some(true),
        'f' => Some(true),
        't' => Some(false),
        'r' => Some(true),
        'n' => Some(false),
        '1' => Some(false),
        'w' => Some(false),
        'v' => Some(false),
        'h' => Some(false),
        'z' => Some(false),
        _ => None,
    }
}

// Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
// the ported reads were elided (see the deferred-I/O / driver notes).
#[allow(unused_assignments, unused_variables)]
fn getopt_long_cgproc(args: &[String]) -> GetoptResult {
    let mut events = Vec::new();
    let mut i = 1usize; // getopt starts after argv[0]
    let mut stop = false;

    while i < args.len() {
        let arg = &args[i];
        if stop || !arg.starts_with('-') || arg == "-" {
            break;
        }
        if arg == "--" {
            i += 1;
            stop = true;
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            // --name or --name=value
            let (name, inline_val) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            match long_option(name) {
                Some((letter, requires)) => {
                    if requires {
                        let val = if let Some(v) = inline_val {
                            Some(v)
                        } else if i + 1 < args.len() {
                            i += 1;
                            Some(args[i].clone())
                        } else {
                            return GetoptResult {
                                events,
                                optind: i,
                                error: true,
                            };
                        };
                        events.push((letter, val));
                    } else {
                        // no_argument: optarg == NULL (even if handler atoi()s it).
                        events.push((letter, None));
                    }
                    i += 1;
                }
                None => {
                    return GetoptResult {
                        events,
                        optind: i,
                        error: true,
                    };
                }
            }
        } else {
            // -abc clustered short options
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                match short_requires_arg(c) {
                    Some(true) => {
                        // argument is the rest of this arg, or the next arg
                        let rest: String = chars[j + 1..].iter().collect();
                        if !rest.is_empty() {
                            events.push((c, Some(rest)));
                        } else if i + 1 < args.len() {
                            i += 1;
                            events.push((c, Some(args[i].clone())));
                        } else {
                            return GetoptResult {
                                events,
                                optind: i,
                                error: true,
                            };
                        }
                        break; // consumed rest of the cluster
                    }
                    Some(false) => {
                        events.push((c, None));
                        j += 1;
                    }
                    None => {
                        return GetoptResult {
                            events,
                            optind: i,
                            error: true,
                        };
                    }
                }
            }
            i += 1;
        }
    }

    GetoptResult {
        events,
        optind: i,
        error: false,
    }
}

/// Faithful `atoi` over an optional argument (`Option<&str>`): `atoi(NULL)` is UB
/// in C. Here the NULL-analogue (`None`) yields `0` (the most common real-world
/// result of `atoi(NULL)` when it does not crash) so the port stays memory-safe
/// while still routing the `--sections` long form through the "wrong" path. The
/// UB is documented at the call site.
fn atoi(arg: Option<&str>) -> i32 {
    match arg {
        // atoi parses a leading integer prefix, defaulting to 0.
        Some(s) => {
            let s = s.trim_start();
            let mut end = 0;
            let bytes = s.as_bytes();
            let mut idx = 0;
            if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
                idx += 1;
            }
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
                end = idx;
            }
            s[..end].parse().unwrap_or(0)
        }
        None => 0, // atoi(NULL): UB — see NOTE.
    }
}

// [spec:cg3:def:cg-proc.main-fn]
// [spec:cg3:sem:cg-proc.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_proc(args: &[String]) -> i32 {
    let mut trace = false;
    let mut wordform_case = false;
    let mut print_word_forms = true;
    let mut delimit_lexical_units = true;
    let mut surface_readings = false;
    let mut only_first = false;
    let mut cmd: char = '\0';
    let mut sections: i32 = 0;
    let mut stream_format: i32 = 1;
    let mut single_rule = String::new();

    // UErrorCode status = U_ZERO_ERROR; (dropped ICU init below)

    let prog = args.first().map(|s| s.as_str()).unwrap_or("cg-proc");

    let getopt = getopt_long_cgproc(args);
    if getopt.error {
        end_program(prog);
    }
    for (c, optarg) in &getopt.events {
        match c {
            'd' => {
                if cmd == '\0' {
                    cmd = 'd';
                } else {
                    end_program(prog);
                }
            }
            'f' => {
                stream_format = atoi(optarg.as_deref());
            }
            't' => trace = true,
            'r' => {
                // single_rule = optarg (strdup). For the long `--rule` no_argument
                // form optarg is NULL → empty; the short `-r X` supplies it.
                single_rule = optarg.clone().unwrap_or_default();
            }
            's' => {
                // BUG (reproduced): for `--sections` (long, no_argument) optarg is
                // NULL → atoi(NULL) is UB. The short `-s N` supplies a value.
                sections = atoi(optarg.as_deref());
            }
            'n' => print_word_forms = false,
            'g' => {
                delimit_lexical_units = false;
                surface_readings = true;
            }
            '1' => only_first = true,
            'w' => wordform_case = true,
            'v' => {
                println!(
                    "VISL CG-3 Disambiguator version {}.{}.{}.{}",
                    CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH, CG3_REVISION
                );
                std::process::exit(0); // EXIT_SUCCESS
            }
            'z' => { /* Null-flush is default */ }
            _ => end_program(prog), // 'h' and default
        }
    }

    // ICU init / codepage / locale dropped (UTF-8 port).

    // Owned option tables.
    let mut options = options();
    let mut options_default = options_default();
    let mut options_override = options_override();
    let mut grammar_options_default = grammar_options_default();
    let mut grammar_options_override = grammar_options_override();

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

    // Grammar grammar; — owned by the parser, moved out after parse.
    let optind = getopt.optind;

    // if (optind <= argc-1) { fopen argv[optind]; read 4 bytes } else endProgram.
    if optind >= args.len() {
        end_program(prog);
    }
    let grammar_path = &args[optind];
    let mut head = [0u8; 4];
    {
        let mut in_ = match std::fs::File::open(grammar_path) {
            Ok(f) => f,
            Err(_) => end_program(prog),
        };
        if in_.read_exact(&mut head).is_err() {
            tracing::error!("Error: Error reading first 4 bytes from grammar!");
            cg3_quit(1, None, 0);
        }
    }

    // Optional input/output files (optind+1 / optind+2). NOTE the C++ open
    // checks are `!stream || stream->bad()` — an ifstream/ofstream that FAILS to
    // open sets failbit, not badbit, so `bad()` is false and the C++ proceeds
    // with a dead stream (input reads as empty; output writes are discarded).
    // The port mirrors that: unreadable input → empty; uncreatable output → sink.
    let input_path: Option<&String> = args.get(optind + 1);
    let output_path: Option<&String> = args.get(optind + 2);

    // Parse the grammar (binary → BinaryGrammar; text → TextualParser + warning).
    let mut grammar: Grammar = if is_cg3b(head) {
        let mut parser = BinaryGrammar::binary_grammar(Grammar::default());
        if parser.parse_grammar_filename(grammar_path) != 0 {
            tracing::error!("Error: Grammar could not be parsed - exiting!");
            cg3_quit(1, None, 0);
        }
        parser.grammar
    } else {
        tracing::warn!(
            "Warning: Text grammar detected - to better process textual\ngrammars, use `vislcg3'; to compile this grammar, use `cg-comp'"
        );
        let mut parser = TextualParser::new(Grammar::default(), false);
        let buffer = match std::fs::read(grammar_path) {
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
        parser.grammar
    };

    // grammar.reindex();
    grammar.reindex(false, false);

    // Grammar cmdargs → parse_opts into grammar_options_{default,override}.
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

    // Build the applicator for the chosen stream format. The ported base owns its
    // grammar (setGrammar takes no arg), so the parsed grammar is moved into the
    // base at construction, then set_grammar() seeds the begin/end/subst tags.
    enum Applicator {
        Base(GrammarApplicator),
        Apertium(ApertiumApplicator),
        Matxin(MatxinApplicator),
        /// The binary applicator borrows the base for the run (wave 4), so the
        /// enum holds the base and the wrapper is built at the run site.
        Binary(GrammarApplicator),
    }

    let mut app = if stream_format == 0 {
        Applicator::Base(GrammarApplicator::new(grammar))
    } else if stream_format == 2 {
        let base = GrammarApplicator::new(grammar);
        let mut m = MatxinApplicator::new(base);
        m.set_null_flush(true);
        m.wordform_case = wordform_case;
        m.print_word_forms = print_word_forms;
        m.print_only_first = only_first;
        Applicator::Matxin(m)
    } else if stream_format == 3 {
        Applicator::Binary(GrammarApplicator::new(grammar))
    } else {
        let base = GrammarApplicator::new(grammar);
        let mut a = ApertiumApplicator::new(base);
        a.wordform_case = wordform_case;
        a.print_word_forms = print_word_forms;
        a.print_only_first = only_first;
        a.delimit_lexical_units = delimit_lexical_units;
        a.surface_readings = surface_readings;
        Applicator::Apertium(a)
    };

    // applicator->setGrammar(&grammar); setOptions(); sections; trace; unicode_tags; unique_tags.
    let base: &mut GrammarApplicator = match &mut app {
        Applicator::Base(b) => b,
        Applicator::Apertium(a) => &mut a.base,
        Applicator::Matxin(m) => &mut m.base,
        Applicator::Binary(b) => b,
    };
    base.set_grammar();
    // setOptions() (C++ default conv=nullptr) reads the `options` table.
    base.set_options(&options);
    for i in 1..=sections {
        base.sections.push(i as u32);
    }
    base.trace = trace;
    base.unicode_tags = true;
    base.unique_tags = false;

    // -r single rule: match rule name and push its number into valid_rules.
    if !single_rule.is_empty() {
        let n = base.grammar.rule_by_number.capacity();
        for i in 0..n {
            if let Some(rule) = base.grammar.rule_by_number.try_get(i) {
                if rule.name == single_rule {
                    let number = rule.number;
                    base.valid_rules.insert_sorted(number);
                }
            }
        }
    }

    // Input stream. Files are slurped into a seekable Cursor. Stdin must stay
    // STREAMING (the C++ reads std::cin incrementally; null-flush clients expect
    // a response per '\0' while the pipe is still open), so it is wrapped in an
    // adapter supporting the only Seek the drivers perform: the ≤3-byte
    // `ux_strip_bom` rewind (SeekFrom::Current with a small negative offset).
    let mut cursor: Box<dyn ReadSeek> = match input_path {
        Some(path) => Box::new(std::io::Cursor::new(
            std::fs::read(path).unwrap_or_default(),
        )),
        None => Box::new(StreamingStdin::new()),
    };
    // ux_stdout: argv[optind+2] if given (create failure → silent sink, per the
    // C++ bad()-never-fires NOTE above), else stdout.
    let mut out: Box<dyn std::io::Write> = match output_path {
        Some(path) => match std::fs::File::create(path) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()),
        },
        None => Box::new(std::io::stdout()),
    };

    // try { switch (cmd) { case 'd': default: runGrammarOnText(...); } }
    match app {
        Applicator::Apertium(mut a) => {
            a.run_grammar_on_text(&mut cursor, &mut out);
        }
        Applicator::Matxin(mut m) => {
            m.run_grammar_on_text(&mut cursor, &mut out);
        }
        Applicator::Base(mut b) => {
            b.run_grammar_on_text(&mut cursor, &mut out);
        }
        Applicator::Binary(mut b) => {
            // Most-derived object is the BinaryApplicator: binary print vtable.
            let mut fmt = crate::binary_applicator::BinaryFormat::default();
            BinaryApplicator::new(&mut b).run_grammar_on_text(&mut fmt, &mut cursor, &mut out);
        }
    }

    // u_cleanup dropped. C++ main falls off the end (implicit 0).
    0
}

/// Object-safe `Read + Seek` so file (Cursor) and stdin (streaming) inputs share
/// one variable; `Box<dyn ReadSeek>` itself impls `Read`/`Seek` via std's
/// blanket `Box` impls, satisfying the generic `run_grammar_on_text` bounds.
trait ReadSeek: Read + std::io::Seek {}
impl<T: Read + std::io::Seek> ReadSeek for T {}

/// Streaming stdin with tiny pushback, standing in for the C++ `std::cin`
/// istream. `Read` pulls straight from `Stdin` (internally buffered, returns as
/// soon as bytes are available on the pipe — no read-to-EOF). `Seek` supports
/// only what `ux_strip_bom` does: `SeekFrom::Current(-n)` for the last few bytes
/// read (istream `putback`); everything else is unsupported.
struct StreamingStdin {
    inner: std::io::Stdin,
    /// Most recent bytes read (bounded), so small rewinds can be replayed.
    history: Vec<u8>,
    /// How many history bytes have been "put back" and must be re-served.
    pushback: usize,
}

const STDIN_HISTORY: usize = 8;

impl StreamingStdin {
    fn new() -> Self {
        StreamingStdin {
            inner: std::io::stdin(),
            history: Vec::new(),
            pushback: 0,
        }
    }
}

impl Read for StreamingStdin {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.pushback > 0 {
            let start = self.history.len() - self.pushback;
            let n = self.pushback.min(buf.len());
            buf[..n].copy_from_slice(&self.history[start..start + n]);
            self.pushback -= n;
            return Ok(n);
        }
        let n = self.inner.read(buf)?;
        self.history.extend_from_slice(&buf[..n]);
        if self.history.len() > STDIN_HISTORY {
            let excess = self.history.len() - STDIN_HISTORY;
            self.history.drain(..excess);
        }
        Ok(n)
    }
}

impl std::io::Seek for StreamingStdin {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match pos {
            std::io::SeekFrom::Current(back) if back <= 0 => {
                let back = (-back) as usize;
                if back + self.pushback > self.history.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "stdin pushback exceeds retained history",
                    ));
                }
                self.pushback += back;
                Ok(0)
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "stdin is not seekable",
            )),
        }
    }
}
