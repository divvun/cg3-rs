//! Loading a compiled grammar and running it, for a program rather than a
//! command line.
//!
//! [`CompiledGrammar::from_cg3b`] loads a `.cg3b` from bytes into a core that
//! every run of it shares; [`CompiledGrammar::run`] runs it on one input, from
//! bytes to bytes, as vislcg3 runs it on one standard input. The typed
//! [`RunOptions`] are turned into vislcg3's option table and merged with the
//! options the grammar carries in `CMDARGS` and `CMDARGS-OVERRIDE`, so a run
//! sees the options vislcg3 would. [`CompiledGrammar::prepare`] and
//! [`PreparedRun::run`] are what `run` is made of, and what vislcg3 calls with
//! the table it merged itself.
//!
//! ```
//! use cg3::embed::{CompiledGrammar, RunOptions};
//! # use cg3::{binary_grammar::BinaryGrammar, grammar::GrammarDraft, textual_parser::TextualParser};
//! # let mut parser = TextualParser::new(GrammarDraft::default(), false);
//! # parser.parse_grammar_utf8(b"DELIMITERS = \"<.>\" ; SELECT (n) ;")?;
//! # let mut cg3b = Vec::new();
//! # BinaryGrammar::new(parser.grammar.finish()?).write_binary_grammar(&mut cg3b)?;
//! let grammar = CompiledGrammar::from_cg3b(&cg3b)?;
//! let input = b"\"<word>\"\n\t\"word\" n\n\t\"word\" v\n";
//! let output = grammar.run(input, &RunOptions::default())?;
//! assert_eq!(String::from_utf8(output)?, "\"<word>\"\n\t\"word\" n\n");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::Write;
use std::sync::Arc;

use crate::binary_grammar::BinaryGrammar;
use crate::error::Cg3Error;
use crate::format_converter::FormatConverter;
use crate::grammar::{Grammar, GrammarCore, GrammarNumbered};
use crate::grammar_applicator::{GrammarApplicator, STDIN_SOURCE_NAME, StreamFormatKind};
use crate::options::{Opt, OptionsTable};
use crate::profiler::Profiler;

// [spec:cg3:req:embed.load]
/// A finished grammar, shared by every run of it.
#[derive(Clone)]
pub struct CompiledGrammar {
    core: Arc<GrammarCore>,
}

// [spec:cg3:req:embed.info]
/// What vislcg3's verbose summary reports about a loaded grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarInfo {
    pub sections: usize,
    pub templates: usize,
    pub rules: usize,
    pub sets: usize,
    pub tags: usize,
    /// Rules no tag index can skip.
    pub rules_any: usize,
    pub has_dependency_rules: bool,
    pub has_relation_rules: bool,
}

/// A stream format, as vislcg3's `--in-*` and `--out-*` options name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Cg,
    Niceline,
    Apertium,
    Fst,
    Plain,
    Jsonl,
    Binary,
}

// [spec:cg3:req:embed.run-options]
/// The options a run reads, one field per vislcg3 option, named after its long
/// form. A flag left `false`, or a value left `None`, is that option left off
/// the command line. An option that takes an optional value is given without
/// one as `Some(String::new())`.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    /// `--in-*`; `None` reads CG, as vislcg3 does.
    pub input: Option<Format>,
    /// `--out-*`; `None` writes CG, as vislcg3 does.
    pub output: Option<Format>,
    pub ordered: bool,
    /// `--unsafe`.
    pub unsafe_mode: bool,
    pub sections: Option<String>,
    pub rules: Option<String>,
    pub rule: Option<String>,
    pub debug_rules: Option<String>,
    pub verbose: Option<String>,
    pub no_mappings: bool,
    pub no_corrections: bool,
    pub no_before_sections: bool,
    pub no_sections: bool,
    pub no_after_sections: bool,
    pub trace: Option<String>,
    pub trace_name_only: bool,
    pub trace_no_removed: bool,
    pub trace_encl: bool,
    pub deleted: bool,
    pub single_run: bool,
    pub max_runs: Option<u32>,
    pub unicode_tags: bool,
    pub unique_tags: bool,
    pub print_ids: bool,
    pub print_dep: bool,
    pub num_windows: Option<u32>,
    pub always_span: bool,
    pub soft_limit: Option<u32>,
    pub hard_limit: Option<u32>,
    pub text_delimit: Option<String>,
    pub dep_delimit: Option<String>,
    pub dep_absolute: bool,
    pub dep_original: bool,
    pub dep_allow_loops: bool,
    pub dep_no_crossing: bool,
    pub no_magic_readings: bool,
    pub no_pass_origin: bool,
    pub split_mappings: bool,
    pub show_end_tags: bool,
    pub no_break: bool,
}

impl RunOptions {
    /// The vislcg3 option table these options are: what is set occurs, with its
    /// value; everything else is as `crate::options::options()` leaves it.
    pub fn to_table(&self) -> OptionsTable {
        let mut table = crate::options::options();
        let mut flag = |o: Opt, on: bool| {
            if on {
                table[o as usize].does_occur = true;
            }
        };
        flag(Opt::Ordered, self.ordered);
        flag(Opt::Unsafe, self.unsafe_mode);
        flag(Opt::Nomappings, self.no_mappings);
        flag(Opt::Nocorrections, self.no_corrections);
        flag(Opt::Nobeforesections, self.no_before_sections);
        flag(Opt::Nosections, self.no_sections);
        flag(Opt::Noaftersections, self.no_after_sections);
        flag(Opt::TraceNameOnly, self.trace_name_only);
        flag(Opt::TraceNoRemoved, self.trace_no_removed);
        flag(Opt::TraceEncl, self.trace_encl);
        flag(Opt::PipeDeleted, self.deleted);
        flag(Opt::Singlerun, self.single_run);
        flag(Opt::UnicodeTags, self.unicode_tags);
        flag(Opt::UniqueTags, self.unique_tags);
        flag(Opt::PrintIds, self.print_ids);
        flag(Opt::PrintDep, self.print_dep);
        flag(Opt::AlwaysSpan, self.always_span);
        flag(Opt::DepAbsolute, self.dep_absolute);
        flag(Opt::DepOriginal, self.dep_original);
        flag(Opt::DepAllowLoops, self.dep_allow_loops);
        flag(Opt::DepBlockCrossing, self.dep_no_crossing);
        flag(Opt::MagicReadings, self.no_magic_readings);
        flag(Opt::NoPassOrigin, self.no_pass_origin);
        flag(Opt::SplitMappings, self.split_mappings);
        flag(Opt::ShowEndTags, self.show_end_tags);
        flag(Opt::NoBreak, self.no_break);
        if let Some(f) = self.input {
            flag(input_option(f), true);
        }
        if let Some(o) = self.output.and_then(output_option) {
            flag(o, true);
        }
        let mut value = |o: Opt, v: Option<String>| {
            if let Some(v) = v {
                table[o as usize].does_occur = true;
                table[o as usize].value = v;
            }
        };
        value(Opt::Sections, self.sections.clone());
        value(Opt::Rules, self.rules.clone());
        value(Opt::Rule, self.rule.clone());
        value(Opt::DebugRules, self.debug_rules.clone());
        value(Opt::Verbose, self.verbose.clone());
        value(Opt::Trace, self.trace.clone());
        value(Opt::TextDelimit, self.text_delimit.clone());
        value(Opt::DepDelimit, self.dep_delimit.clone());
        value(Opt::Maxruns, self.max_runs.map(|n| n.to_string()));
        value(Opt::NumWindows, self.num_windows.map(|n| n.to_string()));
        value(Opt::SoftLimit, self.soft_limit.map(|n| n.to_string()));
        value(Opt::HardLimit, self.hard_limit.map(|n| n.to_string()));
        table
    }
}

fn input_option(f: Format) -> Opt {
    match f {
        Format::Cg => Opt::InCg,
        Format::Niceline => Opt::InNiceline,
        Format::Apertium => Opt::InApertium,
        Format::Fst => Opt::InFst,
        Format::Plain => Opt::InPlain,
        Format::Jsonl => Opt::InJsonl,
        Format::Binary => Opt::InBinary,
    }
}

/// vislcg3 has no `--out-cg`: CG is what it writes when no other is named.
fn output_option(f: Format) -> Option<Opt> {
    match f {
        Format::Cg => None,
        Format::Niceline => Some(Opt::OutNiceline),
        Format::Apertium => Some(Opt::OutApertium),
        Format::Fst => Some(Opt::OutFst),
        Format::Plain => Some(Opt::OutPlain),
        Format::Jsonl => Some(Opt::OutJsonl),
        Format::Binary => Some(Opt::OutBinary),
    }
}

/// What a run is given beside its options.
#[derive(Default)]
pub struct RunContext {
    /// What runtime errors call the input; `None` is a stream with no file
    /// behind it, as vislcg3's standard input is.
    pub input_name: Option<String>,
    /// A profiler the run records into; [`RunReport::profiler`] hands it back.
    pub profiler: Option<Profiler>,
}

/// What a finished run gives back beside its output.
pub struct RunReport {
    /// Whether the input stream carried relations, which `--grammar-bin`
    /// folds into the grammar it writes.
    pub stream_relations: bool,
    pub profiler: Option<Profiler>,
}

// [spec:cg3:req:embed.runs-independent]
/// A run set up over a grammar and its options, waiting for its input. It
/// runs once: a fresh applicator over the shared core, so nothing one run
/// learns reaches the next.
pub struct PreparedRun {
    applicator: FormatConverter,
}

impl CompiledGrammar {
    /// Loads a `.cg3b` and finishes it.
    pub fn from_cg3b(bytes: &[u8]) -> Result<CompiledGrammar, Cg3Error> {
        let mut parser = BinaryGrammar::new(GrammarNumbered::default());
        parser.parse_grammar_buffer(bytes)?;
        Ok(CompiledGrammar::from(parser.grammar.finish()?))
    }

    /// The finished grammar every run shares.
    pub fn core(&self) -> &Arc<GrammarCore> {
        &self.core
    }

    pub fn info(&self) -> GrammarInfo {
        let g = &self.core;
        GrammarInfo {
            sections: g.sections.len(),
            templates: g.templates.len(),
            rules: g.rule_by_number.capacity() as usize,
            sets: g.sets_list.capacity() as usize,
            tags: g.single_tags().size(),
            rules_any: g.rules_any.as_ref().map_or(0, |r| r.size()),
            has_dependency_rules: g.has_dep,
            has_relation_rules: g.has_relations,
        }
    }

    // [spec:cg3:req:embed.run]
    // [spec:cg3:req:embed.grammar-options]
    /// Runs the grammar on one whole input and returns the whole output.
    pub fn run(&self, input: &[u8], options: &RunOptions) -> Result<Vec<u8>, Cg3Error> {
        let mut table = options.to_table();
        crate::options_parser::merge_grammar_options(
            &mut table,
            &self.core.cmdargs,
            &self.core.cmdargs_override,
            None,
        );
        let mut output = Vec::new();
        self.prepare(&table, RunContext::default())?
            .run(input, &mut output)?;
        Ok(output)
    }

    // [spec:cg3:req:embed.cli-runs-through+1]
    /// Sets a run up from an option table that has already been merged, in the
    /// order vislcg3 does, so a bad option fails here, before any input is read.
    pub fn prepare(
        &self,
        options: &OptionsTable,
        context: RunContext,
    ) -> Result<PreparedRun, Cg3Error> {
        let occ = |o: Opt| options[o as usize].does_occur;
        let mut applicator = FormatConverter::new(GrammarApplicator::new(Grammar::default()))?;
        let base = applicator.base_mut();
        base.cfg.fmt_input = [
            (Opt::InCg, StreamFormatKind::Cg),
            (Opt::InNiceline, StreamFormatKind::Niceline),
            (Opt::InApertium, StreamFormatKind::Apertium),
            (Opt::InFst, StreamFormatKind::Fst),
            (Opt::InPlain, StreamFormatKind::Plain),
            (Opt::InJsonl, StreamFormatKind::Jsonl),
            (Opt::InBinary, StreamFormatKind::Binary),
        ]
        .into_iter()
        .find(|&(o, _)| occ(o))
        .map_or(StreamFormatKind::Cg, |(_, f)| f);
        base.grammar = Grammar::from_core(Arc::clone(&self.core));
        base.set_grammar()?;
        base.set_options(options)?;
        base.cfg.input_name = context
            .input_name
            .unwrap_or_else(|| STDIN_SOURCE_NAME.to_string());
        base.cfg.fmt_output = [
            (Opt::OutApertium, StreamFormatKind::Apertium),
            (Opt::OutFst, StreamFormatKind::Fst),
            (Opt::OutNiceline, StreamFormatKind::Niceline),
            (Opt::OutPlain, StreamFormatKind::Plain),
            (Opt::OutJsonl, StreamFormatKind::Jsonl),
            (Opt::OutBinary, StreamFormatKind::Binary),
        ]
        .into_iter()
        .find(|&(o, _)| occ(o))
        .map_or(StreamFormatKind::Cg, |(_, f)| f);
        // As vislcg3 does once the options are read.
        if base.cfg.fmt_output == StreamFormatKind::Apertium {
            base.cfg.unicode_tags = true;
        }
        base.diag.profiler = context.profiler;
        Ok(PreparedRun { applicator })
    }
}

impl PreparedRun {
    // [spec:cg3:req:embed.cli-runs-through+1]
    /// Runs on one whole input, writing the output as it goes.
    pub fn run<W: Write>(mut self, input: &[u8], output: &mut W) -> Result<RunReport, Cg3Error> {
        let mut cursor = std::io::Cursor::new(input);
        self.applicator.run_grammar_on_text(&mut cursor, output)?;
        Ok(RunReport {
            stream_relations: self.applicator.base().cfg.stream_relations,
            profiler: self.applicator.base_mut().diag.profiler.take(),
        })
    }
}

impl From<GrammarCore> for CompiledGrammar {
    fn from(core: GrammarCore) -> CompiledGrammar {
        CompiledGrammar {
            core: Arc::new(core),
        }
    }
}

impl From<Arc<GrammarCore>> for CompiledGrammar {
    fn from(core: Arc<GrammarCore>) -> CompiledGrammar {
        CompiledGrammar { core }
    }
}
