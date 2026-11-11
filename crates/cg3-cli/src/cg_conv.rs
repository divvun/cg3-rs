//! Port of `src/cg-conv.cpp` — the stream format converter.
//!
//! Parses cg-conv options into the library's [`cg3::ConvertOptions`] and
//! converts stdin to stdout through [`cg3::embed::convert_into`], which
//! [`cg3::convert`] is made of.
//!
//! DIVERGENCE: `-M` / `--out-matxin` is not offered. The C++ output-format
//! switch had no `case OUT_MATXIN`, so `-M` silently emitted CG (neither
//! FormatConverter has a Matxin arm); the option is removed rather than carried
//! as a no-op. See plan node `option-wiring`.

use super::parse_opts_env;
use crate::options_conv::{Opt, options_conv, options_default, options_override};
use cg3::ConvertOptions;
use cg3::arg_parser::parse_args;
use cg3::embed::Format;

use super::{EXIT_FAILURE, EXIT_SUCCESS, emit_usage, fail, to_argv};

// [spec:cg3:def:cg-conv.main-fn+2]
// [spec:cg3:sem:cg-conv.main-fn+2]
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
        out.push_str(" CG3_CONV_DEFAULT: Sets default cmdline options, which the actual passed options will override.\n");
        out.push_str(" CG3_CONV_OVERRIDE: Sets forced cmdline options, which will override any passed option.\n");
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

        out.push_str(
            "\n\nKeys for JSONL format (one JSON object per line; full spec in the manual):\n",
        );
        out.push_str(
            "===============================================================================\n",
        );
        out.push_str("Cohort:                     Reading:                   Stream command:\n");
        out.push_str(
            "    w  wordform, no \"<>\"        l  baseform, no \"\"       cmd  <STREAMCMD:...>\n",
        );
        out.push_str("   wb  word-bound blank        ts  tags, incl. mapping\n");
        out.push_str("  sts  static tags              s  sub-reading         Plain text:\n");
        out.push_str(
            "   rs  readings                tr  rule trace              t  one line of text\n",
        );
        out.push_str("  drs  deleted readings\n");
        out.push_str("   ds  dependency self\n");
        out.push_str("   dp  dependency parent\n");
        out.push_str("   id  cohort ID\n");
        out.push_str(" rels  relations\n");
        out.push_str("    z  text after cohort\n");
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
    let options = convert_options(&options_conv, numbers);

    // The ported drivers need `R: Read + Seek`, and stdin is not seekable, so
    // the whole stream is buffered first (faithful for the char-by-char state
    // machines the applicators run).
    let mut input = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut input);

    // [spec:cg3:req:embed.convert]
    // applicator.runGrammarOnText(*instream, std::cout);
    if let Err(e) = cg3::embed::convert_into(&input, &options, &mut std::io::stdout()) {
        return fail(&e);
    }

    // C++ main returns nothing on this path (implicit 0).
    EXIT_SUCCESS
}

/// The library's options for the cg-conv command line in `table`.
///
/// Of several `--in-*` or `--out-*` options, the first cg-conv checks wins:
/// `--in-cg` (or `--in-cg2`), `--in-niceline`, `--in-apertium`, `--in-fst`,
/// `--in-plain`, `--in-jsonl`, `--in-binary`; `--out-apertium`, `--out-fst`,
/// `--out-niceline`, `--out-plain`, `--out-jsonl`, `--out-binary`, with CG
/// when none is given.
fn convert_options(
    table: &crate::options_conv::ConvOptionsTable,
    numbers: NumericOptions,
) -> ConvertOptions {
    let occ = |o: Opt| table[o as usize].does_occur;
    let first = |formats: &[(Opt, Format)]| formats.iter().find(|&&(o, _)| occ(o)).map(|&(_, f)| f);
    let input = if occ(Opt::InCg2) {
        Some(Format::Cg)
    } else {
        first(&[
            (Opt::InCg, Format::Cg),
            (Opt::InNiceline, Format::Niceline),
            (Opt::InApertium, Format::Apertium),
            (Opt::InFst, Format::Fst),
            (Opt::InPlain, Format::Plain),
            (Opt::InJsonl, Format::Jsonl),
            (Opt::InBinary, Format::Binary),
        ])
    };
    let output = first(&[
        (Opt::OutApertium, Format::Apertium),
        (Opt::OutFst, Format::Fst),
        (Opt::OutNiceline, Format::Niceline),
        (Opt::OutPlain, Format::Plain),
        (Opt::OutJsonl, Format::Jsonl),
        (Opt::OutBinary, Format::Binary),
    ]);
    let value = |o: Opt| occ(o).then(|| table[o as usize].value.clone());
    ConvertOptions {
        input,
        in_auto: occ(Opt::InAuto),
        add_tags: occ(Opt::AddTags),
        output,
        // C++ converts the option value and takes buf[0]; UTF-8 port: first
        // char, and buf[0] of an empty conversion is its NUL terminator.
        prefix: value(Opt::MappingPrefix).map(|p| p.chars().next().unwrap_or('\0')),
        wfactor: numbers.wfactor,
        wtag: value(Opt::FstWtag),
        sub_delim: value(Opt::SubDelimiter),
        ltr: occ(Opt::SubLtr),
        ordered: occ(Opt::Ordered),
        parse_dep: occ(Opt::ParseDep),
        dep_delimit: numbers.dep_delimit,
        unicode_tags: occ(Opt::UnicodeTags),
        deleted: occ(Opt::PipeDeleted),
        no_break: occ(Opt::NoBreak),
    }
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
