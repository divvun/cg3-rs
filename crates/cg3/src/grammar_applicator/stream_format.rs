//! The stream-format print vtable (wave 4, `w4-stream-format-trait`).
//!
//! C++ dispatches the driver's `printCohort` / `printSingleWindow` /
//! `printStreamCommand` / `printPlainTextLine` calls through the
//! virtual-inheritance vtable: the
//! most-derived applicator (MweSplitApplicator, FormatConverter, …) overrides
//! the print slots while the shared `GrammarApplicator` base runs the drivers.
//! The wave-2 literal port modelled those vtable slots as flags on the base
//! (`mwe_split_at_print`, `print_dispatch`, `bin_header_done`) plus
//! `fmt_output` switches inside the base printers. Wave 4 replaces all of
//! that with this strategy trait: the drivers
//! ([`run_grammar_on_text_with`](super::GrammarApplicator::run_grammar_on_text_with) /
//! [`run_grammar_on_window_with`](Engine::run_grammar_on_window_with) and the per-format input
//! drivers) take a `&mut impl StreamFormat` and route every print through it.
//!
//! Strategies:
//! * [`CgFormat`] — the base CG text format (the C++ base-class virtuals).
//! * `MweSplitFormat` (in `mwesplit_applicator`) — the MweSplit
//!   `printSingleWindow` override.
//! * `BinaryFormat` (in `binary_applicator`) — the binary stream writers,
//!   owning the `header_done` latch the literal port had hoisted onto the
//!   base as a `Cell`.
//! * `ConvFormat` (in `format_converter`) — the FormatConverter overrides:
//!   a runtime dispatch on `fmt_output`.

use std::io::Write;

use crate::arena::{CohortId, SwId};

use super::Engine;

/// The C++ print vtable: the four print slots the engine drivers dispatch
/// through. `e` is the shared engine base (the C++ `GrammarApplicator`
/// subobject) as the split-borrow [`Engine`] view — every driver that dispatches
/// through a `StreamFormat` is now an `impl Engine<'_>` method that already holds
/// the split view; strategy state (e.g. the binary header latch) lives on the
/// strategy value itself.
pub trait StreamFormat {
    /// Virtual `printCohort(Cohort*, std::ostream&, bool)`.
    fn print_cohort<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    );

    /// Virtual `printSingleWindow(SingleWindow*, std::ostream&, bool)`.
    fn print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    );

    /// Virtual `printStreamCommand(UStringView, std::ostream&)`.
    fn print_stream_command<W: Write>(&mut self, e: &mut Engine<'_>, cmd: &str, output: &mut W);

    /// Virtual `printPlainTextLine(UStringView, std::ostream&)`.
    fn print_plain_text_line<W: Write>(&mut self, e: &mut Engine<'_>, line: &str, output: &mut W);
}

/// The base CG text format — the C++ `GrammarApplicator` print virtuals
/// (no override in the vtable).
#[derive(Default)]
pub struct CgFormat;

impl StreamFormat for CgFormat {
    fn print_cohort<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        let trace = e.cfg.trace;
        e.print_cohort(cohort, output, profiling, trace);
    }

    fn print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        let trace = e.cfg.trace;
        e.print_single_window(window, output, profiling, trace);
    }

    fn print_stream_command<W: Write>(&mut self, e: &mut Engine<'_>, cmd: &str, output: &mut W) {
        e.print_stream_command(cmd, output);
    }

    fn print_plain_text_line<W: Write>(&mut self, e: &mut Engine<'_>, line: &str, output: &mut W) {
        e.print_plain_text_line(line, output);
    }
}
