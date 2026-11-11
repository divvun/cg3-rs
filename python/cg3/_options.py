"""The options a run reads, those a load applies, and those cg-conv and
cg-proc read, each as one immutable value."""

from __future__ import annotations

from dataclasses import dataclass, fields

from cg3._cg3 import (
    Format,
    ProcFormat,
    _check_convert_options,
    _check_load_options,
    _check_proc_options,
    _check_run_options,
)


# [spec:cg3:req:python.load-options]
@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class LoadOptions:
    """The options vislcg3 applies to a grammar while it loads it, each named
    after its vislcg3 long option in snake_case: ``--nrules-v X`` is
    ``nrules_v="X"``. ``nrules`` keeps only the rules whose names its pattern
    matches and ``nrules_v`` leaves out those whose names its pattern matches;
    ``vislcg_compat`` reads a textual grammar as VISLCG2 did; ``prefix`` is the
    mapping prefix, one character, which a ``.cg3b`` must have been compiled
    with. A ``--prefix`` in the grammar's CMDARGS applies as it does in vislcg3.

    The fields are checked when the options are made: a pattern must compile
    as vislcg3 compiles it.
    """

    nrules: str | None = None
    nrules_v: str | None = None
    vislcg_compat: bool = False
    prefix: str | None = None

    def __post_init__(self) -> None:
        _check_load_options(self)

    def __repr__(self) -> str:
        """Names only the options given, as a call that makes them would."""
        given = ", ".join(
            f"{f.name}={getattr(self, f.name)!r}"
            for f in fields(self)
            if getattr(self, f.name) != f.default
        )
        return f"LoadOptions({given})"


# [spec:cg3:req:python.apply+1]
@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class RunOptions:
    """The options a run reads, each named after its vislcg3 long option in
    snake_case: ``--num-windows 4`` is ``num_windows=4``. ``input_format`` and
    ``output_format`` stand for the ``--in-*`` and ``--out-*`` options. An
    option whose value vislcg3 makes optional is ``True`` without one, as
    ``--trace`` is ``trace=True``. Numbers are whole numbers from 0 to
    4294967295.

    The fields are checked when the options are made, so one RunOptions serves
    any number of runs, and ``dataclasses.replace`` makes a variant. The
    options a grammar sets with CMDARGS apply as they do in vislcg3.
    """

    input_format: Format | None = None
    output_format: Format | None = None
    ordered: bool = False
    unsafe: bool = False
    sections: str | None = None
    rules: str | None = None
    rule: str | None = None
    debug_rules: str | None = None
    verbose: bool | str = False
    no_mappings: bool = False
    no_corrections: bool = False
    no_before_sections: bool = False
    no_sections: bool = False
    no_after_sections: bool = False
    trace: bool | str = False
    trace_name_only: bool = False
    trace_no_removed: bool = False
    trace_encl: bool = False
    deleted: bool = False
    single_run: bool = False
    max_runs: int | None = None
    unicode_tags: bool = False
    unique_tags: bool = False
    print_ids: bool = False
    print_dep: bool = False
    num_windows: int | None = None
    always_span: bool = False
    soft_limit: int | None = None
    hard_limit: int | None = None
    text_delimit: bool | str = False
    dep_delimit: bool | str = False
    dep_absolute: bool = False
    dep_original: bool = False
    dep_allow_loops: bool = False
    dep_no_crossing: bool = False
    no_magic_readings: bool = False
    no_pass_origin: bool = False
    split_mappings: bool = False
    show_end_tags: bool = False
    no_break: bool = False

    def __post_init__(self) -> None:
        _check_run_options(self)

    def __repr__(self) -> str:
        """Names only the options given, as a call that makes them would."""
        given = ", ".join(
            f"{f.name}={getattr(self, f.name)!r}"
            for f in fields(self)
            if getattr(self, f.name) != f.default
        )
        return f"RunOptions({given})"


# [spec:cg3:req:python.tools]
@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class ConvertOptions:
    """cg-conv's options, each named after its long option in snake_case:
    ``--sub-delim "#"`` is ``sub_delim="#"``. ``input_format`` and
    ``output_format`` stand for the ``--in-*`` and ``--out-*`` options: the
    input's format is detected from the input when ``input_format`` is left
    out, as cg-conv detects it, and the output is CG. ``in_auto`` detects the
    format even where ``input_format`` or ``add_tags`` names one; ``add_tags``
    gives each word of plain text a minimal reading. ``prefix`` is the mapping
    prefix, one character; ``wfactor`` is what FST weights are multiplied by,
    ``wtag`` the tag they are written as and ``sub_delim`` the characters that
    separate FST sub-readings. ``rtl`` is cg-conv's default, which ``ltr``
    overrides, so it has no effect. ``dep_delimit=True`` is ``--dep-delimit``
    without a value, which cg-conv reads as 10. Numbers are whole numbers
    from 0 to 4294967295, but for ``wfactor``.

    The fields are checked when the options are made.
    """

    input_format: Format | None = None
    in_auto: bool = False
    add_tags: bool = False
    output_format: Format | None = None
    prefix: str | None = None
    wfactor: float | None = None
    wtag: str | None = None
    sub_delim: str | None = None
    rtl: bool = False
    ltr: bool = False
    ordered: bool = False
    parse_dep: bool = False
    dep_delimit: bool | int = False
    unicode_tags: bool = False
    deleted: bool = False
    no_break: bool = False

    def __post_init__(self) -> None:
        _check_convert_options(self)

    def __repr__(self) -> str:
        """Names only the options given, as a call that makes them would.
        ``dep_delimit=0`` is given, though 0 equals False."""
        given = ", ".join(
            f"{f.name}={value!r}"
            for f in fields(self)
            if (value := getattr(self, f.name)) != f.default or type(value) is not type(f.default)
        )
        return f"ConvertOptions({given})"


# [spec:cg3:req:python.tools]
@dataclass(frozen=True, kw_only=True, slots=True, repr=False)
class ProcOptions:
    """cg-proc's options, each named after its long option in snake_case:
    ``--wordform-case`` is ``wordform_case=True``. ``stream_format`` is the
    stream's format by name, which ``--stream-format`` numbers 0, 1 and 2; the
    binary format, 3, is left out. ``sections`` runs only the first that many
    sections and ``rule`` only the rules of that name; ``wordform_case`` gives
    each baseform the case of its wordform, ``no_word_forms`` prints no
    cohort's wordform, ``generation`` leaves out the ``^`` and ``$`` around
    lexical units and prints readings as surface forms, and ``first`` prints
    only the first of the readings left. Disambiguation is all cg-proc does
    and it always flushes on a null byte, so ``disambiguation`` and
    ``null_flush`` have no effect. ``sections`` is a whole number from 0 to
    4294967295.

    The fields are checked when the options are made. The options a grammar
    sets with CMDARGS apply as they do in cg-proc.
    """

    stream_format: ProcFormat = "apertium"
    sections: int | None = None
    rule: str | None = None
    trace: bool = False
    wordform_case: bool = False
    no_word_forms: bool = False
    generation: bool = False
    first: bool = False
    disambiguation: bool = False
    null_flush: bool = False

    def __post_init__(self) -> None:
        _check_proc_options(self)

    def __repr__(self) -> str:
        """Names only the options given, as a call that makes them would."""
        given = ", ".join(
            f"{f.name}={getattr(self, f.name)!r}"
            for f in fields(self)
            if getattr(self, f.name) != f.default
        )
        return f"ProcOptions({given})"
