//! Port of `src/cg-proc.cpp` — the Apertium/Matxin/Binary stream processor.
//!
//! Unlike the other tools, cg-proc parses its own options with POSIX `getopt`
//! (NOT the shared `ArgOption` tables), then loads a grammar and runs
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
//! The command line becomes the library's [`cg3::embed::ProcOptions`], and the
//! grammar runs through [`cg3::embed::CompiledGrammar::prepare_proc`] and
//! [`cg3::embed::PreparedProc::run`], the two steps
//! [`cg3::embed::CompiledGrammar::proc`] is made of, so that stdin streams.

use std::io::Read;

use cg3::binary_grammar::BinaryGrammar;
use cg3::embed::{CompiledGrammar, ProcFormat, ProcOptions};
use cg3::grammar::{GrammarCore, GrammarDraft, GrammarNumbered};
use cg3::inlines::is_cg3b;
use cg3::options::{options, options_default, options_override};
use cg3::options_parser::{merge_grammar_options, merge_options};
use cg3::textual_parser::TextualParser;

use super::{
    EXIT_FAILURE, basename, divvun_version, divvun_version_line, emit, fail, parse_opts_env,
};

// [spec:cg3:def:cg-proc.end-program-fn+3]
// [spec:cg3:sem:cg-proc.end-program-fn+3]
/// C++ `void endProgram(char* name)`. Prints the version + full usage banner and
/// yields `EXIT_FAILURE` for its caller to return. The `HAVE_GETOPT_LONG`
/// variant of the banner is used (the ported getopt below is the long variant).
fn end_program(name: &str) -> i32 {
    let name = basename(name);
    let usage = format!(
        "{}{name}: process a stream with a constraint grammar\n\
         USAGE: {name} [-t] [-s] [-d] [-g] [-r rule] grammar_file [input_file [output_file]]\n\
         {USAGE_OPTIONS}",
        divvun_version_line("Disambiguator"),
    );
    emit(std::io::stdout(), &usage);
    EXIT_FAILURE
}

/// The option list [`end_program`] prints below its usage line.
const USAGE_OPTIONS: &str = concat!(
    "Options:\n",
    "\t-d, --disambiguation:\t morphological disambiguation\n",
    "\t-s, --sections=NUM:\t specify number of sections to process\n",
    "\t-f, --stream-format=NUM: set the format of the I/O stream to NUM,\n",
    "\t\t\t\t   where `0' is VISL format, `1' is Apertium\n",
    "\t\t\t\t   format, `2` is Matxin, and `3` is binary\n",
    "                  (default: 1)\n",
    "\t-r, --rule=NAME:\t run only the named rule\n",
    "\t-t, --trace:\t\t print debug output on stderr\n",
    "\t-w, --wordform-case:\t enforce surface case on lemma/baseform \n",
    "\t\t\t\t   (to work with -w option of lt-proc)\n",
    "\t-n, --no-word-forms:\t do not print out the word form of each cohort\n",
    "\t-g, --generation:\t do not surround lexical units in ^$\n",
    "\t-1, --first:\t \t only output the first analysis if ambiguity remains\n",
    "\t-z, --null-flush:\tflush output on the null character\n",
    "\t-v, --version:\t \t version\n",
    "\t-h, --help:\t\t show this help\n",
);

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

fn getopt_long_cgproc(args: &[String]) -> GetoptResult {
    let mut events = Vec::new();
    let mut i = 1usize; // getopt starts after argv[0]

    while i < args.len() {
        let arg = &args[i];
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        if arg == "--" {
            // getopt's "stop scanning" marker: everything after is non-options.
            i += 1;
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

    // A numeric value that is no number refuses the command line like any
    // other malformed option, after saying which.
    let error = match check_number_args(&events) {
        Ok(()) => false,
        Err(e) => {
            tracing::error!("{e}");
            true
        }
    };
    GetoptResult {
        events,
        optind: i,
        error,
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

/// A numeric argument that is not a whole number.
#[derive(Debug, thiserror::Error)]
#[error("Error: -{flag} expects a whole number, not \"{value}\"")]
struct NumberArgError {
    flag: char,
    value: String,
}

// [spec:cg3:req:robustness.cli-arguments]
/// Check the values `-f` and `-s` will be `atoi`'d from. DIVERGENCE: C `atoi`
/// reads a value that is no number as 0 and cuts one with trailing text down to
/// its digits; both are refused here. A missing value — the long `--sections`,
/// declared without an argument, whose `atoi(NULL)` is undefined in C — still
/// reads as 0.
fn check_number_args(events: &[(char, Option<String>)]) -> Result<(), NumberArgError> {
    for (flag, arg) in events {
        if let ('f' | 's', Some(value)) = (flag, arg)
            && value.trim().parse::<i32>().is_err()
        {
            return Err(NumberArgError {
                flag: *flag,
                value: value.clone(),
            });
        }
    }
    Ok(())
}

/// Parses the grammar at `grammar_path`, whose first bytes are `head` (binary
/// → BinaryGrammar; text → TextualParser + warning), and finishes it where the
/// C++ calls `grammar.reindex()`. `Err` is the exit code, the failure already
/// reported.
fn load_grammar(grammar_path: &str, head: [u8; 4]) -> Result<GrammarCore, i32> {
    let finished = if is_cg3b(head) {
        let mut parser = BinaryGrammar::new(GrammarNumbered::default());
        parser
            .parse_grammar_filename(grammar_path)
            .map_err(|e| fail(&e))?;
        parser.grammar.finish()
    } else {
        tracing::warn!(
            "Warning: Text grammar detected - to better process textual\ngrammars, use `vislcg3'; to compile this grammar, use `cg-comp'"
        );
        let mut parser = TextualParser::new(GrammarDraft::default(), false);
        let buffer = std::fs::read(grammar_path).map_err(|_| {
            tracing::error!("Error: Error opening {} for reading!", grammar_path);
            EXIT_FAILURE
        })?;
        // [spec:cg3:req:diagnostics.source-named]
        parser
            .parse_grammar_named(&buffer, grammar_path)
            .map_err(|e| fail(&e))?;
        parser.grammar.finish()
    };
    finished.map_err(|e| fail(&e))
}

// [spec:cg3:def:cg-proc.main-fn+1]
// [spec:cg3:sem:cg-proc.main-fn+1]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_proc(args: &[String]) -> i32 {
    let mut proc = ProcOptions::default();
    let mut cmd: char = '\0';

    let prog = args.first().map(|s| s.as_str()).unwrap_or("cg-proc");

    let getopt = getopt_long_cgproc(args);
    if getopt.error {
        return end_program(prog);
    }
    for (c, optarg) in &getopt.events {
        match c {
            'd' if cmd == '\0' => cmd = 'd',
            'd' => return end_program(prog),
            'f' => {
                proc.stream_format = match atoi(optarg.as_deref()) {
                    0 => ProcFormat::Cg,
                    2 => ProcFormat::Matxin,
                    3 => ProcFormat::Binary,
                    _ => ProcFormat::Apertium,
                };
            }
            't' => proc.trace = true,
            'r' => {
                // single_rule = optarg (strdup). For the long `--rule` no_argument
                // form optarg is NULL → empty; the short `-r X` supplies it.
                proc.rule = optarg.clone();
            }
            's' => {
                // BUG (reproduced): for `--sections` (long, no_argument) optarg is
                // NULL → atoi(NULL) is UB. The short `-s N` supplies a value.
                proc.sections = u32::try_from(atoi(optarg.as_deref())).ok();
            }
            'n' => proc.no_word_forms = true,
            'g' => proc.generation = true,
            '1' => proc.first = true,
            'w' => proc.wordform_case = true,
            'v' => {
                emit(std::io::stdout(), &divvun_version("Disambiguator"));
                // C++ exit(EXIT_SUCCESS) — returned to the wrapper in src/bin,
                // which owns the only process::exit.
                return 0;
            }
            'z' => { /* Null-flush is default */ }
            _ => return end_program(prog), // 'h' and default
        }
    }

    // Owned option tables.
    let mut options = options();
    let mut options_default = options_default();
    let mut options_override = options_override();

    parse_opts_env("CG3_DEFAULT", &mut options_default);
    parse_opts_env("CG3_OVERRIDE", &mut options_override);
    merge_options(&mut options, &options_default, &options_override, None);

    // Grammar grammar; — owned by the parser, moved out after parse.
    let optind = getopt.optind;

    // if (optind <= argc-1) { fopen argv[optind]; read 4 bytes } else endProgram.
    if optind >= args.len() {
        return end_program(prog);
    }
    let grammar_path = &args[optind];
    let mut head = [0u8; 4];
    {
        let mut in_ = match std::fs::File::open(grammar_path) {
            Ok(f) => f,
            Err(_) => return end_program(prog),
        };
        if in_.read_exact(&mut head).is_err() {
            tracing::error!("Error: Error reading first 4 bytes from grammar!");
            return EXIT_FAILURE;
        }
    }

    // Optional input/output files (optind+1 / optind+2). NOTE the C++ open
    // checks are `!stream || stream->bad()` — an ifstream/ofstream that FAILS to
    // open sets failbit, not badbit, so `bad()` is false and the C++ proceeds
    // with a dead stream (input reads as empty; output writes are discarded).
    // The port mirrors that: unreadable input → empty; uncreatable output → sink.
    let input_path: Option<&String> = args.get(optind + 1);
    let output_path: Option<&String> = args.get(optind + 2);

    let grammar = match load_grammar(grammar_path, head) {
        Ok(g) => CompiledGrammar::from(g),
        Err(code) => return code,
    };

    merge_grammar_options(
        &mut options,
        &grammar.core().cmdargs,
        &grammar.core().cmdargs_override,
        Some(&options_override),
    );

    // [spec:cg3:req:embed.proc]
    // applicator->setGrammar(&grammar); setOptions(); sections; trace; unicode_tags; unique_tags.
    let run = match grammar.prepare_proc(&proc, &options, input_path.cloned()) {
        Ok(run) => run,
        Err(e) => return fail(&e),
    };

    // Input stream. Files are slurped into a seekable Cursor. Stdin must stay
    // STREAMING (the C++ reads std::cin incrementally; null-flush clients expect
    // a response per '\0' while the pipe is still open), so it is wrapped in an
    // adapter supporting the only Seek the drivers perform: the ≤3-byte
    // `strip_bom` rewind (SeekFrom::Current with a small negative offset).
    let mut cursor: Box<dyn ReadSeek> = match input_path {
        Some(path) => Box::new(std::io::Cursor::new(
            std::fs::read(path).unwrap_or_default(),
        )),
        None => Box::new(StreamingStdin::new()),
    };
    // Output: argv[optind+2] if given (create failure → silent sink, per the
    // C++ bad()-never-fires NOTE above), else stdout.
    let mut out: Box<dyn std::io::Write> = match output_path {
        Some(path) => match std::fs::File::create(path) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()),
        },
        None => Box::new(std::io::stdout()),
    };

    // try { switch (cmd) { case 'd': default: runGrammarOnText(...); } }
    if let Err(e) = run.run(&mut cursor, &mut out) {
        return fail(&e);
    }

    // C++ main falls off the end (implicit 0).
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
/// only what `strip_bom` does: `SeekFrom::Current(-n)` for the last few bytes
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
