//! Port of `src/cg-conv.cpp` — the stream format converter.
//!
//! Parses cg-conv options, configures a
//! [`crate::format_converter::FormatConverter`] (via its public shared-base
//! accessors — the composition analogue of the C++ public inheritance), and
//! runs it over stdin→stdout. stdin is buffered into a seekable Cursor because
//! the ported drivers need `R: Read + Seek`. FST/plaintext-only options are
//! stored on the converter's persistent format strategies.
//!
//! DIVERGENCE: `-M` / `--out-matxin` is not offered. The C++ output-format
//! switch had no `case OUT_MATXIN`, so `-M` silently emitted CG (neither
//! FormatConverter has a Matxin arm); the option is removed rather than carried
//! as a no-op. See plan node `option-wiring`.

use crate::arg_parser::parse_args;
use crate::options_conv::{Opt, options_conv, options_default, options_override};
use crate::options_parser::parse_opts_env;

use super::{EXIT_FAILURE, EXIT_SUCCESS, emit_usage, fail, to_argv};

// [spec:cg3:def:cg-conv.main-fn+1]
// [spec:cg3:sem:cg-conv.main-fn+1]
/// C++ `int main(int argc, char* argv[])`.
// faithful port: the C++ `for (i=0; i<NUM_OPTIONS_CONV; ++i)` scans cover the
// whole table — its length IS the enum constant (`ConvOptionsTable`).
pub fn main_conv(args: &[String]) -> i32 {
    // Owned local option tables (the C++ globals are mutated in place).
    let mut options_conv = options_conv();
    let mut options_default = options_default();
    let mut options_override = options_override();

    let mut argv = to_argv(args);
    let argc = parse_args(
        argv.len() as i32,
        &mut argv,
        Opt::NumOptionsConv as i32,
        &mut options_conv,
    );

    // parse_opts_env("CG3_CONV_DEFAULT", options_default); / OVERRIDE.
    parse_opts_env("CG3_CONV_DEFAULT", &mut options_default);
    parse_opts_env("CG3_CONV_OVERRIDE", &mut options_override);
    for (conv, (def, ovr)) in options_conv
        .iter_mut()
        .zip(options_default.iter().zip(options_override.iter()))
    {
        if def.does_occur && !conv.does_occur {
            *conv = def.clone();
        }
        if ovr.does_occur {
            *conv = ovr.clone();
        }
    }

    let occ = |opts: &crate::options_conv::ConvOptionsTable, o: Opt| opts[o as usize].does_occur;

    if argc < 0 || occ(&options_conv, Opt::Help1) || occ(&options_conv, Opt::Help2) {
        // FILE* out = (argc < 0) ? stderr : stdout;
        let mut out = String::new();
        out.push_str("Usage: cg-conv [OPTIONS]\n");
        out.push('\n');
        out.push_str("Environment variable:\n");
        out.push_str(" CG3_CONV_DEFAULT: Sets default cmdline options_conv, which the actual passed options_conv will override.\n");
        out.push_str(" CG3_CONV_OVERRIDE: Sets forced cmdline options_conv, which will override any passed option.\n");
        out.push('\n');
        out.push_str("Options:\n");

        let mut longest = 0usize;
        for o in options_conv.iter() {
            if !o.description.is_empty() {
                longest = longest.max(o.long_name.map_or(0, |s| s.len()));
            }
        }
        for o in options_conv.iter() {
            let desc = &o.description;
            if !desc.is_empty() && !desc.starts_with('!') {
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
                out.push_str(&format!("  {}\n", desc));
            }
        }

        out.push_str("\n\nKeys for JSONL format:\n");
        out.push_str(
            "===============================================================================\n",
        );
        out.push_str("Cohort:                     Reading:                   Stream Command:\n");
        out.push_str(
            "    w  wordform/token          l  lemma/base form        cmd  stream command\n",
        );
        out.push_str("  sts  static tags            ts  tags\n");
        out.push_str("   rs  readings                s  subreading\n");
        out.push_str("  drs  deleted readings                                Plain text:\n");
        out.push_str("   ds  dependency self                                     t  text line\n");
        out.push_str("   dp  dependency parent\n");
        out.push_str("    z  text line(s) suffix\n");
        out.push_str(
            "===============================================================================\n",
        );

        return emit_usage(&out, argc < 0);
    }

    let numbers = match NumericOptions::read(&options_conv) {
        Ok(numbers) => numbers,
        Err(e) => {
            tracing::error!("{e}");
            return EXIT_FAILURE;
        }
    };

    // in-cg2 → in-cg; out-cg2 → out-cg.
    if occ(&options_conv, Opt::InCg2) {
        options_conv[Opt::InCg as usize].does_occur = true;
    }
    if occ(&options_conv, Opt::OutCg2) {
        options_conv[Opt::OutCg as usize].does_occur = true;
    }

    // FormatConverter applicator(std::cerr); Grammar& grammar = applicator.conv_grammar;
    // The C++ sets ORDERED, SUB_LTR and MAPPING_PREFIX on `grammar` further
    // down, after the ctor installed it. An installed grammar is immutable
    // here, so all three are applied as the conv grammar is built instead.
    //
    // DIVERGENCE: `_MPREFIX` is seeded at install time, so it now reports the
    // `--prefix` in effect (as vislcg3's always has) rather than the default.
    let ordered = occ(&options_conv, Opt::Ordered);
    let sub_ltr = occ(&options_conv, Opt::SubLtr);
    // C++ converts the option value and takes buf[0]; UTF-8 port: first char,
    // and buf[0] of an empty conversion is its NUL terminator.
    let mapping_prefix = occ(&options_conv, Opt::MappingPrefix).then(|| {
        options_conv[Opt::MappingPrefix as usize]
            .value
            .chars()
            .next()
            .unwrap_or('\0')
    });
    let base =
        crate::grammar_applicator::GrammarApplicator::new(crate::grammar::Grammar::default());
    let conv = crate::format_converter::FormatConverter::with_conv_grammar(base, |grammar| {
        if ordered {
            grammar.ordered = true;
        }
        if sub_ltr {
            grammar.sub_readings_ltr = true;
        }
        if let Some(mp) = mapping_prefix {
            grammar.mapping_prefix = mp;
        }
    });
    let mut applicator = match conv {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };

    // The C++ strips a BOM from stdin here. The ported drivers need
    // `R: Read + Seek`, and stdin is not seekable, so the whole stream is
    // buffered into a Cursor first (faithful for the char-by-char state
    // machines the applicators run).
    let mut input_bytes = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut input_bytes);
    let mut instream = std::io::Cursor::new(input_bytes);
    crate::uextras::strip_bom(&mut instream);

    // cg3_sformat fmt = CG3SF_INVALID;
    use crate::grammar_applicator::StreamFormatKind;
    let mut fmt = StreamFormatKind::Invalid;

    // if (ADD_TAGS) { options_conv[IN_PLAIN].doesOccur = true; ...add_tags = true; }
    if occ(&options_conv, Opt::AddTags) {
        options_conv[Opt::InPlain as usize].does_occur = true;
        applicator.set_plaintext_add_tags(true);
    }

    if occ(&options_conv, Opt::InCg) {
        fmt = StreamFormatKind::Cg;
    } else if occ(&options_conv, Opt::InNiceline) {
        fmt = StreamFormatKind::Niceline;
    } else if occ(&options_conv, Opt::InApertium) {
        fmt = StreamFormatKind::Apertium;
    } else if occ(&options_conv, Opt::InFst) {
        fmt = StreamFormatKind::Fst;
    } else if occ(&options_conv, Opt::InPlain) {
        fmt = StreamFormatKind::Plain;
    } else if occ(&options_conv, Opt::InJsonl) {
        fmt = StreamFormatKind::Jsonl;
    } else if occ(&options_conv, Opt::InBinary) {
        fmt = StreamFormatKind::Binary;
    }

    if occ(&options_conv, Opt::InAuto) || fmt == StreamFormatKind::Invalid {
        // _instream = applicator.detectFormat(std::cin); fmt = applicator.fmt_input;
        //
        // The C++ wraps the peeked prefix in a replaying bstreambuf and reads on
        // from THAT. Here the input is already a fully-buffered seekable Cursor,
        // so the replay is a seek back to the pre-peek (post-BOM) position —
        // downstream sees the identical stream.
        let pos = instream.position();
        let _wrapped = applicator.detect_format(&mut instream);
        drop(_wrapped);
        instream.set_position(pos);
        fmt = applicator.base().cfg.fmt_input;
    }
    applicator.base_mut().cfg.fmt_input = fmt;

    if occ(&options_conv, Opt::SubDelimiter) {
        let mut sub_delims = options_conv[Opt::SubDelimiter as usize].value.clone();
        sub_delims.push('+');
        applicator.set_fst_sub_delims(sub_delims);
    }
    if occ(&options_conv, Opt::FstWtag) {
        applicator.set_fst_wtag(options_conv[Opt::FstWtag as usize].value.clone());
    }
    if let Some(wfactor) = numbers.wfactor {
        applicator.set_fst_wfactor(wfactor);
    }

    // fmt_output selection.
    applicator.base_mut().cfg.fmt_output = StreamFormatKind::Cg;
    if occ(&options_conv, Opt::OutApertium) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Apertium;
        applicator.base_mut().cfg.unicode_tags = true;
    } else if occ(&options_conv, Opt::OutFst) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Fst;
    } else if occ(&options_conv, Opt::OutNiceline) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Niceline;
    } else if occ(&options_conv, Opt::OutPlain) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Plain;
    } else if occ(&options_conv, Opt::OutJsonl) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Jsonl;
    } else if occ(&options_conv, Opt::OutBinary) {
        applicator.base_mut().cfg.fmt_output = StreamFormatKind::Binary;
    }

    if occ(&options_conv, Opt::UnicodeTags) {
        applicator.base_mut().cfg.unicode_tags = true;
    }
    if occ(&options_conv, Opt::PipeDeleted) {
        applicator.base_mut().cfg.pipe_deleted = true;
    }
    if occ(&options_conv, Opt::NoBreak) {
        applicator.base_mut().cfg.add_spacing = false;
    }
    if occ(&options_conv, Opt::ParseDep) {
        applicator.base_mut().cfg.parse_dep = true;
        applicator.base_mut().doc.deps.has_dep = true;
    }
    if let Some(dep_delimit) = numbers.dep_delimit {
        applicator.base_mut().cfg.dep_delimit = dep_delimit;
        applicator.base_mut().cfg.parse_dep = true;
    }
    applicator.base_mut().cfg.is_conv = true;
    applicator.base_mut().cfg.trace = true;
    applicator.base_mut().cfg.verbosity_level = 0;

    // applicator.runGrammarOnText(*instream, std::cout);
    let mut stdout = std::io::stdout();
    if let Err(e) = applicator.run_grammar_on_text(&mut instream, &mut stdout) {
        return fail(&e);
    }

    // C++ main returns nothing on this path (implicit 0).
    EXIT_SUCCESS
}

/// The cg-conv options whose values are numbers, read before the run starts.
struct NumericOptions {
    /// `-W` / `--wfactor`.
    wfactor: Option<f64>,
    /// `--dep-delimit`; 10 when it is given without a value.
    dep_delimit: Option<u32>,
}

/// An option value that is not the number the option names.
#[derive(Debug, thiserror::Error)]
#[error("Error: --{option} expects {expected}, not \"{value}\"")]
struct OptionValueError {
    option: &'static str,
    expected: &'static str,
    value: String,
}

impl NumericOptions {
    // [spec:cg3:req:robustness.cli-arguments]
    /// Read `-W` and `--dep-delimit` from the merged option table — the command
    /// line and `CG3_CONV_DEFAULT` / `CG3_CONV_OVERRIDE` alike.
    ///
    /// DIVERGENCE: the C++ converts them with `std::stod` / `std::stoul` where
    /// it applies them, so a value that is no number throws and ends the
    /// process, and one that only starts with a number is cut down to it. Here
    /// the whole value must be a number the field can hold, and anything else
    /// is refused before stdin is read.
    fn read(options: &crate::options_conv::ConvOptionsTable) -> Result<Self, OptionValueError> {
        let wfactor = option_number(options, Opt::FstWfactor, "a number")?;
        let dep = &options[Opt::DepDelimit as usize];
        let dep_delimit = if dep.does_occur && dep.value.is_empty() {
            Some(10)
        } else {
            option_number(options, Opt::DepDelimit, "a whole number up to 4294967295")?
        };
        Ok(NumericOptions {
            wfactor,
            dep_delimit,
        })
    }
}

/// The value of `opt` as a `T`, or `None` when the option was not given.
fn option_number<T: std::str::FromStr>(
    options: &crate::options_conv::ConvOptionsTable,
    opt: Opt,
    expected: &'static str,
) -> Result<Option<T>, OptionValueError> {
    let option = &options[opt as usize];
    if !option.does_occur {
        return Ok(None);
    }
    option
        .value
        .trim()
        .parse()
        .map(Some)
        .map_err(|_| OptionValueError {
            option: option.long_name.unwrap_or_default(),
            expected,
            value: option.value.clone(),
        })
}
