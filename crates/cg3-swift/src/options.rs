//! The options Swift passes, as the records the Swift package fills from its
//! `LoadOptions`, `ApplyOptions`, `ConvertOptions` and `ProcOptions`, and the
//! library's options they are.

use cg3::embed::{Format, ProcFormat, ProcOptions, RunOptions};

use crate::error::Cg3Error;

// [spec:cg3:req:swift.apply]
/// Every stream format but the binary one, which a string cannot carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StreamFormat {
    Cg,
    Niceline,
    Apertium,
    Fst,
    Plain,
    Jsonl,
}

impl From<StreamFormat> for Format {
    fn from(format: StreamFormat) -> Format {
        match format {
            StreamFormat::Cg => Format::Cg,
            StreamFormat::Niceline => Format::Niceline,
            StreamFormat::Apertium => Format::Apertium,
            StreamFormat::Fst => Format::Fst,
            StreamFormat::Plain => Format::Plain,
            StreamFormat::Jsonl => Format::Jsonl,
        }
    }
}

// [spec:cg3:req:swift.load-options]
/// The options vislcg3 applies to a grammar while it loads it.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct LoadOptions {
    pub nrules: Option<String>,
    pub nrules_v: Option<String>,
    pub vislcg_compat: bool,
    /// One character, which Swift gives as a `Unicode.Scalar`.
    pub prefix: Option<String>,
}

// [spec:cg3:req:swift.load-options]
/// The library's load options, refusing a pattern that does not compile as
/// vislcg3 compiles it.
pub fn load_options(given: LoadOptions) -> Result<cg3::embed::LoadOptions, Cg3Error> {
    for (pattern, flag) in [(&given.nrules, "--nrules"), (&given.nrules_v, "--nrules-v")] {
        if let Some(pattern) = pattern {
            cg3::embed::rule_filter(pattern, flag).map_err(Cg3Error::invalid_argument)?;
        }
    }
    Ok(cg3::embed::LoadOptions {
        prefix: prefix(given.prefix.as_deref())?,
        nrules: given.nrules,
        nrules_v: given.nrules_v,
        vislcg_compat: given.vislcg_compat,
    })
}

/// A mapping prefix, which must be one character.
pub fn prefix(given: Option<&str>) -> Result<Option<char>, Cg3Error> {
    let Some(given) = given else {
        return Ok(None);
    };
    let mut chars = given.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(Some(c)),
        _ => Err(Cg3Error::invalid_argument("prefix must be one character")),
    }
}

// [spec:cg3:req:swift.apply]
/// The options a run reads, each a field of the library's `RunOptions`. An
/// option whose value vislcg3 makes optional is `Some("")` when given without
/// one.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct ApplyOptions {
    pub input: Option<StreamFormat>,
    pub output: Option<StreamFormat>,
    pub ordered: bool,
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

impl From<ApplyOptions> for RunOptions {
    fn from(o: ApplyOptions) -> RunOptions {
        RunOptions {
            input: o.input.map(Format::from),
            output: o.output.map(Format::from),
            ordered: o.ordered,
            unsafe_mode: o.unsafe_mode,
            sections: o.sections,
            rules: o.rules,
            rule: o.rule,
            debug_rules: o.debug_rules,
            verbose: o.verbose,
            no_mappings: o.no_mappings,
            no_corrections: o.no_corrections,
            no_before_sections: o.no_before_sections,
            no_sections: o.no_sections,
            no_after_sections: o.no_after_sections,
            trace: o.trace,
            trace_name_only: o.trace_name_only,
            trace_no_removed: o.trace_no_removed,
            trace_encl: o.trace_encl,
            deleted: o.deleted,
            single_run: o.single_run,
            max_runs: o.max_runs,
            unicode_tags: o.unicode_tags,
            unique_tags: o.unique_tags,
            print_ids: o.print_ids,
            print_dep: o.print_dep,
            num_windows: o.num_windows,
            always_span: o.always_span,
            soft_limit: o.soft_limit,
            hard_limit: o.hard_limit,
            text_delimit: o.text_delimit,
            dep_delimit: o.dep_delimit,
            dep_absolute: o.dep_absolute,
            dep_original: o.dep_original,
            dep_allow_loops: o.dep_allow_loops,
            dep_no_crossing: o.dep_no_crossing,
            no_magic_readings: o.no_magic_readings,
            no_pass_origin: o.no_pass_origin,
            split_mappings: o.split_mappings,
            show_end_tags: o.show_end_tags,
            no_break: o.no_break,
        }
    }
}

// [spec:cg3:req:swift.tools]
/// cg-conv's options, each a field of the library's `ConvertOptions`, with
/// `prefix` one character. cg-conv reads `--dep-delimit` without a value as
/// 10, which Swift passes as 10.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct ConvertOptions {
    pub input: Option<StreamFormat>,
    pub in_auto: bool,
    pub add_tags: bool,
    pub output: Option<StreamFormat>,
    pub prefix: Option<String>,
    pub wfactor: Option<f64>,
    pub wtag: Option<String>,
    pub sub_delim: Option<String>,
    pub ltr: bool,
    pub ordered: bool,
    pub parse_dep: bool,
    pub dep_delimit: Option<u32>,
    pub unicode_tags: bool,
    pub deleted: bool,
    pub no_break: bool,
}

// [spec:cg3:req:swift.tools]
/// The library's `ConvertOptions`, refusing a prefix of more than one character.
pub fn convert_options(o: ConvertOptions) -> Result<cg3::ConvertOptions, Cg3Error> {
    Ok(cg3::ConvertOptions {
        input: o.input.map(Format::from),
        in_auto: o.in_auto,
        add_tags: o.add_tags,
        output: o.output.map(Format::from),
        prefix: prefix(o.prefix.as_deref())?,
        wfactor: o.wfactor,
        wtag: o.wtag,
        sub_delim: o.sub_delim,
        ltr: o.ltr,
        ordered: o.ordered,
        parse_dep: o.parse_dep,
        dep_delimit: o.dep_delimit,
        unicode_tags: o.unicode_tags,
        deleted: o.deleted,
        no_break: o.no_break,
    })
}

// [spec:cg3:req:swift.tools]
/// cg-proc's stream formats but the binary one, by name rather than by the
/// number `--stream-format` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProcStreamFormat {
    Cg,
    Apertium,
    Matxin,
}

// [spec:cg3:req:swift.tools]
/// cg-proc's options that change what it writes, each a field of the
/// library's `ProcOptions`.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProcRunOptions {
    pub stream_format: ProcStreamFormat,
    pub sections: Option<u32>,
    pub rule: Option<String>,
    pub trace: bool,
    pub wordform_case: bool,
    pub no_word_forms: bool,
    pub generation: bool,
    pub first: bool,
}

impl From<ProcRunOptions> for ProcOptions {
    fn from(o: ProcRunOptions) -> ProcOptions {
        ProcOptions {
            stream_format: match o.stream_format {
                ProcStreamFormat::Cg => ProcFormat::Cg,
                ProcStreamFormat::Apertium => ProcFormat::Apertium,
                ProcStreamFormat::Matxin => ProcFormat::Matxin,
            },
            sections: o.sections,
            rule: o.rule,
            trace: o.trace,
            wordform_case: o.wordform_case,
            no_word_forms: o.no_word_forms,
            generation: o.generation,
            first: o.first,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:cg3:req:swift.load-options/test]
    #[test]
    fn load_options_refuse_what_vislcg3_would() {
        let pattern = LoadOptions {
            nrules: Some("(".into()),
            ..LoadOptions::default()
        };
        assert!(matches!(
            load_options(pattern),
            Err(Cg3Error::InvalidArgument { .. })
        ));
        let long = LoadOptions {
            prefix: Some("%%".into()),
            ..LoadOptions::default()
        };
        assert_eq!(
            load_options(long).unwrap_err(),
            Cg3Error::invalid_argument("prefix must be one character")
        );
        let given = load_options(LoadOptions {
            nrules: Some("^R".into()),
            prefix: Some("%".into()),
            vislcg_compat: true,
            ..LoadOptions::default()
        })
        .unwrap();
        assert_eq!(given.prefix, Some('%'));
        assert_eq!(given.nrules.as_deref(), Some("^R"));
        assert!(given.vislcg_compat);
    }

    // [spec:cg3:req:swift.tools/test]
    #[test]
    fn proc_formats_are_cg_procs() {
        let options = ProcRunOptions {
            stream_format: ProcStreamFormat::Matxin,
            sections: Some(2),
            rule: None,
            trace: true,
            wordform_case: false,
            no_word_forms: false,
            generation: false,
            first: true,
        };
        let library = ProcOptions::from(options);
        assert_eq!(library.stream_format, ProcFormat::Matxin);
        assert_eq!(library.sections, Some(2));
        assert!(library.trace && library.first);
    }
}
