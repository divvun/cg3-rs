//! Port of `src/cg-conv.cpp` — the stream format converter.
//!
//! Parses cg-conv options, configures a
//! [`crate::format_converter::FormatConverter`] (via its public shared-base
//! accessors — the composition analogue of the C++ public inheritance), and
//! runs it over stdin→stdout. stdin is buffered into a seekable Cursor because
//! the ported drivers need `R: Read + Seek`. Two FST/plaintext-only field
//! assignments are elided with MISMATCH NOTEs at their sites (the fields live
//! on wrappers FormatConverter cannot reach, and those format arms CG3Quit in
//! the current engine, so the values could never be observed).
//!
//! ## Reproduced bug: `-M` / `--out-matxin` (OUT_MATXIN) unhandled
//! The C++ output-format switch has NO `case OUT_MATXIN`, so passing `-M` sets
//! `options_conv[OUT_MATXIN].doesOccur` but never changes `fmt_output` — the
//! converter silently emits CG instead of Matxin. This is faithfully reproduced
//! below: the `fmt_output` selection has no OUT_MATXIN arm (marked).

use crate::icu_uoptions::u_parseArgs;
use crate::options_conv::{OPTIONS, options_conv, options_default, options_override};
use crate::options_parser::parse_opts_env;

use super::{U_ILLEGAL_ARGUMENT_ERROR, U_ZERO_ERROR, to_uargv};

// [spec:cg3:def:cg-conv.main-fn]
// [spec:cg3:sem:cg-conv.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_conv(args: &[String]) -> i32 {
    // UErrorCode status = U_ZERO_ERROR;
    // ICU init dropped (UTF-8 port).

    // Owned local option tables (the C++ globals are mutated in place).
    let mut options_conv = options_conv();
    let mut options_default = options_default();
    let mut options_override = options_override();

    // argc = u_parseArgs(argc, argv, options_conv.size(), options_conv.data());
    let mut argv = to_uargv(args);
    let argc = u_parseArgs(
        argv.len() as i32,
        &mut argv,
        OPTIONS::NUM_OPTIONS_CONV as i32,
        &mut options_conv,
    );

    // parse_opts_env("CG3_CONV_DEFAULT", options_default); / OVERRIDE.
    parse_opts_env("CG3_CONV_DEFAULT", &mut options_default);
    parse_opts_env("CG3_CONV_OVERRIDE", &mut options_override);
    for i in 0..OPTIONS::NUM_OPTIONS_CONV as usize {
        if options_default[i].does_occur && !options_conv[i].does_occur {
            options_conv[i] = options_default[i].clone();
        }
        if options_override[i].does_occur {
            options_conv[i] = options_override[i].clone();
        }
    }

    let occ = |opts: &crate::options_conv::options_conv_t, o: OPTIONS| opts[o as usize].does_occur;

    if argc < 0 || occ(&options_conv, OPTIONS::HELP1) || occ(&options_conv, OPTIONS::HELP2) {
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
        for i in 0..OPTIONS::NUM_OPTIONS_CONV as usize {
            if !options_conv[i].description.is_empty() {
                longest = longest.max(options_conv[i].long_name.map_or(0, |s| s.len()));
            }
        }
        for i in 0..OPTIONS::NUM_OPTIONS_CONV as usize {
            let desc = &options_conv[i].description;
            if !desc.is_empty() && !desc.starts_with('!') {
                out.push(' ');
                if options_conv[i].short_name != '\0' {
                    out.push_str(&format!("-{},", options_conv[i].short_name));
                } else {
                    out.push_str("   ");
                }
                let ln = options_conv[i].long_name.unwrap_or("");
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

        if argc < 0 {
            eprint!("{}", out);
            return U_ILLEGAL_ARGUMENT_ERROR;
        } else {
            print!("{}", out);
            return U_ZERO_ERROR;
        }
    }

    // in-cg2 → in-cg; out-cg2 → out-cg.
    if occ(&options_conv, OPTIONS::IN_CG2) {
        options_conv[OPTIONS::IN_CG as usize].does_occur = true;
    }
    if occ(&options_conv, OPTIONS::OUT_CG2) {
        options_conv[OPTIONS::OUT_CG as usize].does_occur = true;
    }

    // ucnv_setDefaultName / uloc_setDefault dropped (UTF-8 port).

    // FormatConverter applicator(std::cerr);
    let base =
        crate::grammar_applicator::GrammarApplicator::new(crate::grammar::Grammar::default());
    let mut applicator = crate::format_converter::FormatConverter::new(base);

    // Grammar& grammar = applicator.conv_grammar; if (ORDERED) grammar.ordered = true;
    // NOTE: in C++ `conv_grammar` IS the applicator's active grammar; in this
    // port that storage lives in `base.grammar` (`FormatConverter::conv_grammar`
    // is a kept-for-parity placeholder), so grammar settings target `base_mut()`.
    if occ(&options_conv, OPTIONS::ORDERED) {
        applicator.base_mut().grammar.ordered = true;
    }

    // ux_stripBOM(std::cin); — the ported drivers need `R: Read + Seek`, and
    // stdin is not seekable, so the whole stream is buffered into a Cursor first
    // (faithful for the char-by-char state machines the applicators run).
    let mut input_bytes = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut input_bytes);
    let mut instream = std::io::Cursor::new(input_bytes);
    crate::uextras::ux_strip_bom(&mut instream);

    // cg3_sformat fmt = CG3SF_INVALID;
    use crate::grammar_applicator::cg3_sformat;
    let mut fmt = cg3_sformat::CG3SF_INVALID;

    // if (ADD_TAGS) { options_conv[IN_PLAIN].doesOccur = true; ...add_tags = true; }
    if occ(&options_conv, OPTIONS::ADD_TAGS) {
        options_conv[OPTIONS::IN_PLAIN as usize].does_occur = true;
        // dynamic_cast<PlaintextApplicator&>(applicator).add_tags = true;
        // MISMATCH (NOTE): `add_tags` lives on the composition wrapper
        // `PlaintextApplicator`, which FormatConverter cannot reach — its PLAIN
        // input arm routes to CG3Quit() in the current engine, so the flag could
        // never be observed; the assignment is elided.
    }

    if occ(&options_conv, OPTIONS::IN_CG) {
        fmt = cg3_sformat::CG3SF_CG;
    } else if occ(&options_conv, OPTIONS::IN_NICELINE) {
        fmt = cg3_sformat::CG3SF_NICELINE;
    } else if occ(&options_conv, OPTIONS::IN_APERTIUM) {
        fmt = cg3_sformat::CG3SF_APERTIUM;
    } else if occ(&options_conv, OPTIONS::IN_FST) {
        fmt = cg3_sformat::CG3SF_FST;
    } else if occ(&options_conv, OPTIONS::IN_PLAIN) {
        fmt = cg3_sformat::CG3SF_PLAIN;
    } else if occ(&options_conv, OPTIONS::IN_JSONL) {
        fmt = cg3_sformat::CG3SF_JSONL;
    } else if occ(&options_conv, OPTIONS::IN_BINARY) {
        fmt = cg3_sformat::CG3SF_BINARY;
    }

    if occ(&options_conv, OPTIONS::IN_AUTO) || fmt == cg3_sformat::CG3SF_INVALID {
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
        fmt = applicator.base().fmt_input;
    }
    applicator.base_mut().fmt_input = fmt;

    // Grammar& settings — live grammar is base.grammar (see the ORDERED NOTE).
    if occ(&options_conv, OPTIONS::SUB_LTR) {
        applicator.base_mut().grammar.sub_readings_ltr = true;
    }
    if occ(&options_conv, OPTIONS::MAPPING_PREFIX) {
        // C++ converts the option value and takes buf[0]; UTF-8 port: first char.
        applicator.base_mut().grammar.mapping_prefix = options_conv
            [OPTIONS::MAPPING_PREFIX as usize]
            .value
            .chars()
            .next()
            .unwrap();
    }
    // MISMATCH (NOTE): `sub_delims` / `wtag` / `wfactor` live on the composition
    // wrapper `FSTApplicator`, which FormatConverter cannot reach — its FST
    // input/output arms route to CG3Quit() in the current engine, so the values
    // could never be observed; the three assignments (SUB_DELIMITER value + '+',
    // FST_WTAG value, FST_WFACTOR stod) are elided.

    // fmt_output selection. NOTE the reproduced OUT_MATXIN bug: no arm for it.
    applicator.base_mut().fmt_output = cg3_sformat::CG3SF_CG;
    if occ(&options_conv, OPTIONS::OUT_APERTIUM) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_APERTIUM;
        applicator.base_mut().unicode_tags = true;
    } else if occ(&options_conv, OPTIONS::OUT_FST) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_FST;
    } else if occ(&options_conv, OPTIONS::OUT_NICELINE) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_NICELINE;
    } else if occ(&options_conv, OPTIONS::OUT_PLAIN) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_PLAIN;
    } else if occ(&options_conv, OPTIONS::OUT_JSONL) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_JSONL;
    } else if occ(&options_conv, OPTIONS::OUT_BINARY) {
        applicator.base_mut().fmt_output = cg3_sformat::CG3SF_BINARY;
    }
    // BUG (reproduced): `-M` / OUT_MATXIN has no case here, so fmt_output stays CG.

    if occ(&options_conv, OPTIONS::UNICODE_TAGS) {
        applicator.base_mut().unicode_tags = true;
    }
    if occ(&options_conv, OPTIONS::PIPE_DELETED) {
        applicator.base_mut().pipe_deleted = true;
    }
    if occ(&options_conv, OPTIONS::NO_BREAK) {
        applicator.base_mut().add_spacing = false;
    }
    if occ(&options_conv, OPTIONS::PARSE_DEP) {
        applicator.base_mut().parse_dep = true;
        applicator.base_mut().has_dep = true;
    }
    if occ(&options_conv, OPTIONS::DEP_DELIMIT) {
        // std::stoul(value) — throws (→ terminates) on non-numeric; unwrap.
        let v = options_conv[OPTIONS::DEP_DELIMIT as usize].value.clone();
        applicator.base_mut().dep_delimit = if !v.is_empty() {
            v.parse().unwrap()
        } else {
            10
        };
        applicator.base_mut().parse_dep = true;
    }
    applicator.base_mut().is_conv = true;
    applicator.base_mut().trace = true;
    applicator.base_mut().verbosity_level = 0;

    // applicator.runGrammarOnText(*instream, std::cout);
    let mut stdout = std::io::stdout();
    if let Err(e) = applicator.run_grammar_on_text(&mut instream, &mut stdout) {
        crate::error::cg3_exit(e.exit_code());
    }

    // u_cleanup dropped. C++ main returns nothing on this path (implicit 0).
    U_ZERO_ERROR
}
