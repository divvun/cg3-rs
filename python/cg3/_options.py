"""The options a run reads, as one immutable value."""

from __future__ import annotations

from dataclasses import dataclass, fields

from cg3._cg3 import Format, _check_run_options


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
