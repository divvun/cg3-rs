# GrammarApplicator field triage

**Purpose.** `GrammarApplicator` (`crates/cg3/src/grammar_applicator/mod.rs:233`) is
the god object we are decomposing into four cohesive views —
**`EngineConfig` (cfg)**, **`Document` (doc)**, **`RuleScratch` (scratch)**, and
**`Diagnostics` (diag)** — while keeping the ported function inventory 1:1 with the
C++ original. This document classifies every one of its fields into exactly one of
those buckets and inventories the "warts" (fields that look like one thing but
behave like another, or that carry no live behavior).

**Method.** Each field was classified from its actual read/write sites, gathered
with `rg -n '\.<field>\b'` over `crates/cg3/src` + `crates/cg3/tests` plus a
bare-`self` split-self sweep inside `crates/cg3/src/grammar_applicator/`. Writer
sites are cited first (with `file:line` anchors preserved for every writer so the
rehome can be verified), then key readers. Sites on look-alike fields of *other*
structs (`Grammar::has_dep`, `TextualParser::filebase`, `SingleWindow::valid_rules`,
local `variables_*`, method calls that share a field's name, etc.) were excluded.

**Bucket definitions.**
- **cfg** — set during setup (`new` / `set_grammar` / `set_options` / `index` /
  CLI wiring in `src/tools/*` and the format-applicator constructors) and read-only
  during the run. Config content that never changes once a run starts.
- **doc** — run-mutable state whose lifetime is the *document* (the whole stream /
  window set): counters, run-phase latches, the window/store, dependency bookkeeping.
- **scratch** — per-rule / per-window / per-cohort transient state that is cleared
  or reset inside the rule-application loops (`RuleScratch`).
- **diag** — profiler / trace / statistics state (`Diagnostics`).

**Predecided fields** (assigned before this triage, not re-litigated here):
`grammar` stays its own field; `window` (C++ `gWindow`) and `store` → doc;
`profiler` → diag.

## Bucket map

All 128 struct fields in declaration order (`mod.rs:234–420`). The "wart" column
flags fields whose bucket assignment or liveness is discussed in the Wart inventory
below; "—" means clean. Writer `file:line` anchors are load-bearing for the rehome
and are retained verbatim.

| #  | field | bucket | wart | one-line evidence (W = write, R = read) |
|----|-------|--------|------|------------------------------------------|
| 1  | always_span | cfg | — | W set_options core.rs:1534; R run_contextual_test.rs:650,664,678,765,771,777,1075,1105 |
| 2  | apply_mappings | cfg | — | W set_options core.rs:1544,1546; R run_rules/schedule.rs:88 |
| 3  | apply_corrections | cfg | — | W set_options core.rs:1548,1550; R run_rules/schedule.rs:92 |
| 4  | no_before_sections | cfg | — | W set_options core.rs:1552,1554; R run_rules/window.rs:593 |
| 5  | no_sections | cfg | — | W set_options core.rs:1556,1558; R run_rules/window.rs:601 |
| 6  | no_after_sections | cfg | — | W set_options core.rs:1560,1562; R run_rules/window.rs:639 |
| 7  | trace | cfg | SWAPPER | W(setup) set_options core.rs:1572,1578,1582,1586; tools cg_proc.rs:480, cg_conv.rs:267. W(RUN swap) core.rs:1738,1778 (print_debug_rule), core.rs:1792,1809 (add_profiling_example). R core.rs:794,945,990,1039; reflow.rs:1361,1380,1456; run_rules/dispatch.rs, restructure.rs:431,950, single_rule.rs:929 |
| 8  | trace_name_only | cfg | — | W set_options core.rs:1579; R core.rs:747 |
| 9  | trace_no_removed | cfg | — | W set_options core.rs:1583; R core.rs:990,1039 |
| 10 | trace_encl | cfg | — | W set_options core.rs:1587; R run_rules/window.rs:914 |
| 11 | allow_magic_readings | cfg | — | W set_options core.rs:1685; ctors plaintext_applicator.rs:87, format_converter.rs:168; R run_grammar.rs:169, single_rule.rs:448 |
| 12 | no_pass_origin | cfg | — | W set_options core.rs:1688; R single_rule.rs:613 |
| 13 | r#unsafe | cfg | — | W(setup) core.rs:1564,1566 (set_options `--unsafe`); R(RUN) single_rule.rs:300,815, dispatch.rs:103 (gates unsafe-rule execution). **Absent from the part files — recovered here.** |
| 14 | ordered | cfg | — | W set_options core.rs:1569; index core.rs:604 (mirrors grammar.ordered); R reflow.rs:940,1354,1371 |
| 15 | show_end_tags | cfg | — | W set_options core.rs:1694; R core.rs:819; mwesplit_applicator.rs:201 (+ via base) |
| 16 | unicode_tags | cfg | — | W set_options core.rs:1536,1538; tools vislcg3.rs:474, cg_proc.rs:481, cg_conv.rs:229,244; R core.rs:878; niceline_applicator.rs:645 |
| 17 | unique_tags | cfg | — | W set_options core.rs:1540,1542; tools cg_proc.rs:482; R core.rs:825 (+ via base) |
| 18 | dry_run | cfg | WRITE-ONLY | W set_options core.rs:1593; R **none** (port gap: C++ gates reflow/output on it) |
| 19 | owns_grammar | cfg | DEAD | W mwesplit_applicator.rs:101; R **none** (core.rs:259 is a comment; Rust owns grammar by value) |
| 20 | input_eof | doc | LATCH | W(RUN) run_grammar.rs:1168; plaintext_applicator.rs:468, niceline_applicator.rs:502; R reflow.rs:384,564; run_rules/window.rs:1043 |
| 21 | seen_barrier | scratch | — | W(RUN) run_contextual_test.rs:272,292 (=true); single_rule.rs:598, dispatch.rs:1315 (=false reset); R dispatch.rs:1327 |
| 22 | is_conv | cfg | — | W ctors mwesplit_applicator.rs:102; tools cg_conv.rs:266; R matxin/apertium/fst applicators via base |
| 23 | split_mappings | cfg | — | W set_options core.rs:1691; R core.rs:1025 (+ via base) |
| 24 | pipe_deleted | cfg | — | W set_options core.rs:1590; tools cg_conv.rs:247; R run_grammar.rs:830 |
| 25 | add_spacing | cfg | — | W set_options core.rs:1697; tools cg_conv.rs:250; R core.rs:596 (in index) |
| 26 | print_ids | cfg | — | W set_options core.rs:1642; R core.rs:931 |
| 27 | fmt_input | cfg | — | W tools vislcg3.rs:440–454, cg_conv.rs:194; format_converter.rs:231 (detect_format at stream start); R format_converter.rs:254, cg_conv.rs:192 |
| 28 | fmt_output | cfg | — | W tools cg_conv.rs:226–239, vislcg3.rs:471–484; R format_converter.rs:400,422,454,470; run_grammar.rs:566,1194,1196 |
| 29 | dep_has_spanned | doc | LATCH | W(RUN) reflow.rs:366 (one-shot on window-boundary span); binary_applicator.rs:261; R reflow.rs:361; core.rs:882; niceline_applicator.rs:653; binary_applicator.rs:876 |
| 30 | dep_delimit | cfg | — | W set_options core.rs:1665; tools cg_conv.rs:259; R core.rs:606; run_grammar.rs:385,388,699; reflow.rs:384 (+ via base) |
| 31 | dep_absolute | cfg | — | W set_options core.rs:1673; R core.rs:879; niceline_applicator.rs:650 |
| 32 | dep_original | cfg | — | W set_options core.rs:1676; R core.rs:833 (+ via base) |
| 33 | dep_block_loops | cfg | — | W set_options core.rs:1679; R reflow.rs:316 |
| 34 | dep_block_crossing | cfg | — | W set_options core.rs:1682; R reflow.rs:321 |
| 35 | num_windows | cfg | — | W new mod.rs:467 (=2); set_options core.rs:1648; R run_grammar.rs:543,554,765; window.rs:884 (+ via base) |
| 36 | soft_limit | cfg | — | W set_options core.rs:1651; R run_grammar.rs:626,655 (+ via base) |
| 37 | hard_limit | cfg | — | W set_options core.rs:1654; R core.rs:677,688 (dep_span_width); run_grammar.rs:697 (+ via base) |
| 38 | sections | cfg | — | W set_options core.rs:1602; tools cg_proc.rs:478; R index core.rs:632,633,646,648 |
| 39 | valid_rules | cfg | — | W set_options core.rs:1611,1618,1715; index core.rs:672 (line→number remap); tools cg_proc.rs:491; R core.rs:663,667; schedule.rs:38,72, window.rs:132,201, dispatch.rs:312 |
| 40 | trace_rules | cfg | — | W set_options core.rs:1708; R run_rules/schedule.rs:138 |
| 41 | debug_rules | cfg | — | W set_options core.rs:1722; R single_rule.rs:709,729,747 |
| 42 | variables | doc | — | W(RUN) run_grammar.rs:939 (clear),980 (insert); window.rs:1033,1036,1039, dispatch.rs:214,227 (+ via base); R match_set.rs:670, run_grammar.rs:1285, single_rule.rs:1091, dispatch.rs:201 |
| 43 | verbosity_level | cfg | — | W set_options core.rs:1627; tools cg_mwesplit.rs:120, cg_conv.rs:268, vislcg3.rs:292,362; R run_contextual_test.rs:1293,1349; reflow.rs:204,256,507 |
| 44 | debug_level | cfg | WRITE-ONLY | W set_options core.rs:1634 (DODEBUG); R **none** (C++ prints "Debug level set to N" + gates verbose diagnostics — deferred) |
| 45 | section_max_count | cfg | — | W set_options core.rs:1596 (SINGLERUN),1599 (MAXRUNS); R schedule.rs:115, window.rs:613,614 |
| 46 | has_dep | doc | LATCH | W(setup) core.rs:1645 (--print-dep). W(RUN) reflow.rs:1004 (dep tag in stream), dispatch.rs:1419,1449 (SETPARENT/SETCHILD fires); R core.rs:833,856,1207,1223,1248,1253, restructure.rs:480, window.rs:1041 |
| 47 | parse_dep | cfg | — | W(setup) index core.rs:607 (from grammar.has_dep/dep_delimit), set_options core.rs:1670; tools cg_conv.rs:253,264; R reflow.rs:985 |
| 48 | dep_highest_seen | doc | LATCH | W(RUN) reflow.rs:1548,1581, run_grammar.rs:143,681,723,759,900,1131,1178; reset per-window run_grammar.rs:436; R run_grammar.rs:386–388 |
| 49 | window | doc | — | Predecided (C++ `gWindow`, owned document window). |
| 50 | has_relations | doc | LATCH | W(RUN) reflow.rs:1025 (relation tag seen), dispatch.rs:1456 (relation rule fires); no setup-phase writer of the applicator field; R core.rs:836, window.rs:1061 (+ via base) |
| 51 | grammar | (own field) | — | Predecided — stays its own field. |
| 52 | store | doc | — | Predecided (owns the runtime arenas). |
| 53 | profiler | diag | — | W(setup/teardown) tools vislcg3.rs:490 (move in), :514 (take out); R(RUN) single_rule.rs:695,702,736,743, core.rs:1811,1830,1840 |
| 54 | filebase | cfg | DEAD | W ctor only (`None`); R **none** — `filebase()` accessor (core.rs:1890) hardcodes `""`. Faithful port of C++ `nullptr`. |
| 55 | span_pattern_latin | cfg | WRITE-ONLY | W index core.rs:680 (hard_limit-derived, setup); R **none** (dep-span print path deferred) |
| 56 | span_pattern_utf | cfg | WRITE-ONLY | W index core.rs:679 (setup); R **none** (deferred output formatter) |
| 57 | ws | cfg | — | W index core.rs:597 (`ws[2]='\n'` iff `!add_spacing`; pre-guard but idempotent/config-derived); R core.rs:1074, fst_applicator.rs:275, niceline_applicator.rs:758 |
| 58 | numLines | doc | — | W(RUN) run_grammar.rs:1157 (+ applicators); R run_grammar.rs:684,726,802,1134; core.rs:1513,1403,1457,529,580 (error/quit lines) |
| 59 | numWindows | doc | — | W(RUN) run_grammar.rs:434,742 (+ applicators); R run_grammar.rs:769,920 (`is_multiple_of(reset_after)`) |
| 60 | numCohorts | doc | — | W(RUN) run_grammar.rs:688,730,801,1139 (+ applicators); R applicator diagnostics (apertium:1405,1488; matxin:873; fst:994) |
| 61 | numReadings | doc | — | W(RUN) run_grammar.rs:188,375, reflow.rs:1261, restructure.rs:359,883,1153, dispatch.rs:893,968 (+ applicators); R applicators |
| 62 | did_index | cfg | — | W index core.rs:682 (setup); R index core.rs:599 (early-return idempotency guard). Setup-time latch, guards the once-only schedule build. |
| 63 | dep_deep_seen | scratch | — | W(RUN) insert run_contextual_test.rs:1241; cleared single_rule.rs:599, run_contextual_test.rs:603, restructure.rs:40,71,683,784,817, dispatch.rs:1309,1343; R run_contextual_test.rs:1238 |
| 64 | numsections | cfg | — | W index core.rs:646 (=sections.len(), setup); R index core.rs:649 (loop bound) |
| 65 | runsections | cfg | — | W(setup) index core.rs:612,619,626,642,657; R(RUN, read-only/cloned) window.rs:594,607,620,640 |
| 66 | externals | doc | — | W(RUN) dispatch.rs:292,300 (EXTERNAL child procs spawned/reused); R dispatch.rs:269 |
| 67 | ci_depths | scratch | — | W(RUN) run_contextual_test.rs:143 (resize),645,659,673,764,770,776; reset single_rule.rs:600,601; R run_contextual_test.rs:142,644,658,672,763,769,775 |
| 68 | cohortIterators | scratch | — | W(RUN) run_contextual_test.rs:778 insert; R/mut :961,985. Iterator pool keyed by depth. |
| 69 | topologyLeftIters | scratch | — | W(RUN) run_contextual_test.rs:766 insert, :990,992 remove+reinsert; R :963 |
| 70 | topologyRightIters | scratch | — | W(RUN) run_contextual_test.rs:772 insert, :996,998 remove+reinsert; R :967 |
| 71 | depParentIters | scratch | — | W(RUN) run_contextual_test.rs:669 insert, :1002,1004 remove+reinsert; R :970 |
| 72 | depDescendentIters | scratch | — | W(RUN) run_contextual_test.rs:683 insert; R/mut :972,1008 |
| 73 | depAncestorIters | scratch | — | W(RUN) run_contextual_test.rs:655 insert; R/mut :975,1013 |
| 74 | match_single | diag | WRITE-ONLY | W(RUN) match_set.rs:821 (`++` in doesSetMatch); R **none** (C++ verbose match stats — see §2 footnote a) |
| 75 | match_comp | diag | DEAD | No uses anywhere but field def + ctor (C++ companion counter, never incremented) |
| 76 | match_sub | diag | WRITE-ONLY | W(RUN) match_set.rs:1188,1193 (`++`); R **none** (C++ verbose match stats — see §2 footnote a) |
| 77 | begintag | cfg | — | W(setup) core.rs:486 (set_grammar); re-seeded apertium:1115/matxin:752 (post-index, pre-loop; idempotent); R(RUN) run_grammar.rs:124,133, reflow.rs:1535,1540, core.rs:819 |
| 78 | endtag | cfg | — | W(setup) core.rs:487 (set_grammar); re-seeded apertium:1121/matxin:758; R(RUN) run_grammar.rs:398,667,709,908,1116,1186, restructure.rs:638,649,746,751,762, dispatch.rs:339,350,356, reflow.rs:1601, core.rs:819 |
| 79 | substtag | cfg | — | W(setup) core.rs:488 (set_grammar); R(RUN) dispatch.rs:1080 |
| 80 | tag_begin | cfg | — | W(setup) core.rs:483 (set_grammar); R(RUN) run_grammar.rs:121, reflow.rs:1532, matxin:896 (cohort wordform) |
| 81 | tag_end | cfg | WRITE-ONLY | W(setup) core.rs:484 (set_grammar); R **none** (only doc-comment mention reflow.rs:40; run-phase uses the `endtag` hash form) |
| 82 | tag_subst | cfg | WRITE-ONLY | W(setup) core.rs:485 (set_grammar); R **none** (run-phase uses the `substtag` hash form) |
| 83 | par_left_tag | scratch | — | W(RUN) window.rs:830 (matched paren tag), :852,1071 (reset 0); R match_set.rs:742,746 |
| 84 | par_right_tag | scratch | — | W(RUN) window.rs:834, :853,1072 (reset); R match_set.rs:755,759 |
| 85 | par_left_pos | scratch | — | W(RUN) window.rs:838, :854,1073 (reset 0); R run_contextual_test.rs:1453,1461, match_set.rs:749, single_rule.rs:318,322,327 |
| 86 | par_right_pos | scratch | — | W(RUN) window.rs:839,855,1074 (enclosure reflow); R run_contextual_test.rs:1453,1463, single_rule.rs:322,327, match_set.rs:762 |
| 87 | did_final_enclosure | scratch | LATCH | W(RUN) window.rs:856 (latch true), 1009 (reset false per window); R window.rs:851, schedule.rs:96,99 (RF_ENCL_FINAL gating). Per-window phase toggle. |
| 88 | mprefix_key | cfg | — | W(setup) core.rs:492 (set_grammar); R window.rs:1038 |
| 89 | mprefix_value | cfg | — | W(setup) core.rs:494 (set_grammar); R window.rs:1038 |
| 90 | tmpl_cntx | scratch | SWAPPER | W(RUN save/restore) run_contextual_test.rs:450,454,508,511–513,844,854, match_set.rs:1256,1296,1299–1300; full-reset single_rule.rs:603, restructure.rs:41,72,684,785,818, dispatch.rs:1310,1344; R run_contextual_test.rs:375,378,447–449,631,836,846, match_set.rs:1252–1255. Frame state → stays save/restore in RuleScratch. |
| 91 | regexgrps_store | scratch | — | W single_rule.rs:378–379 (resize per cohort), match_set.rs:361,443,554; R reflow.rs:773 |
| 92 | regexgrps_z | scratch | — | W single_rule.rs:371 (clear per cohort),501,721,773; R single_rule.rs:500,720 |
| 93 | regexgrps_c | scratch | — | W single_rule.rs:372 (clear per cohort),499,719,772; R single_rule.rs:498,713,718 |
| 94 | same_basic | scratch | — | W single_rule.rs:543 (per target reading); R match_set.rs:804 (T_SAME_BASIC) |
| 95 | rule_target | scratch | — | W single_rule.rs:544 (reset None),564,577; R match_set.rs:786, core.rs:970,1066 (profiling) |
| 96 | context_target | scratch | WRITE-ONLY | W single_rule.rs:545 (reset None),578, restructure.rs:59,805, dispatch.rs:1331; R **none** (verified — see §2 footnote b and Wart inventory). C++ mirror consumed by attach diagnostics not yet ported. |
| 97 | merge_with | scratch | — | W run_contextual_test.rs:204, restructure.rs:681, single_rule.rs:608 (reset None); R restructure.rs:701,704, single_rule.rs:631,632 |
| 98 | current_rule | scratch | SWAPPER | W schedule.rs:75 (per scheduled rule), dispatch.rs:567,572,584 (save/restore around nested run); R core.rs:1507, match_set.rs:1332. Frame state → stays save/restore in RuleScratch. |
| 99 | context_stack | scratch | — | W single_rule.rs:396 (push per cohort),805,811,818,841,858,864 (pop) + last_mut; R pervasive (match_set.rs, context.rs, run_contextual_test.rs, dispatch.rs, restructure.rs) |
| 100 | cohortsets | scratch | — | W single_rule.rs:60 (push per frame),71 (pop),167 (replace top); R window.rs:161,162 |
| 101 | rocits | scratch | — | W single_rule.rs:63 (push per frame),72 (pop),188,191,196,236,803 (advance); R window.rs:175,187,191, single_rule.rs:168,231,801 |
| 102 | readings_plain | scratch | — | W single_rule.rs:367 (clear per cohort),752 (insert cache); R single_rule.rs:482,483 |
| 103 | text_delimiters | cfg | MRU | W(setup) core.rs:526 (set_grammar compile),551,578 (set_text_delimiter); W(RUN) run_grammar.rs:1104 via test_string_against `rxs.swap(0,i)` MRU move-to-front; R same. Content is cfg; run-time reorder is a pure cache optimization. |
| 104 | unif_tags_rs | scratch | — | W single_rule.rs:373 (clear per cohort),531,533; R single_rule.rs:506 |
| 105 | unif_tags_store | scratch | — | W single_rule.rs:382–383 (resize per cohort),536, match_set.rs:1202,1399; R context.rs:133, window.rs:423, single_rule.rs:657, match_set.rs:1343 |
| 106 | unif_sets_rs | scratch | — | W single_rule.rs:374 (clear per cohort),532,534; R single_rule.rs:507 |
| 107 | unif_sets_store | scratch | — | W single_rule.rs:386–387 (resize per cohort),537, match_set.rs:1412; R match_set.rs:1094,1112,1118,1124,1344, window.rs:401, single_rule.rs:660 |
| 108 | unif_last_wordform | scratch | — | W single_rule.rs:540 (reset per target reading), match_set.rs:610; R match_set.rs:605,606 |
| 109 | unif_last_baseform | scratch | — | W single_rule.rs:541 (reset per target reading), match_set.rs:594; R match_set.rs:589,590 |
| 110 | unif_last_textual | scratch | — | W single_rule.rs:542 (reset per target reading), match_set.rs:636; R match_set.rs:631,632 |
| 111 | rule_hits | scratch | — | W window.rs:896 (clear per window), schedule.rs:123 (init 0), restructure.rs:184 (increment); R restructure.rs:184 |
| 112 | ss_taglist | scratch | DEAD | W/R **none** in src or tests (only ctor `ScopedStack::new()` mod.rs:558). Faithful port of an unexercised C++ pool. |
| 113 | ss_utags | scratch | — | R match_set.rs:1319 (`.get()` pool checkout, internally mutates); no explicit push/reset. Load-bearing RAII pool (mutation hidden in `get()`). |
| 114 | ss_usets | scratch | — | R match_set.rs:1320 (`.get()` pool checkout); no explicit push/reset. As ss_utags. |
| 115 | ss_u32sv | scratch | — | R match_set.rs:1128, reflow.rs:599 (`.get()` pool checkout). Live pool (unlike ss_taglist). **Absent from the part files — recovered here.** |
| 116 | index_regexp_yes | scratch | — | W core.rs:294 (reset_indexes clear), match_set.rs:365,447; R match_set.rs:337,427 |
| 117 | index_regexp_no | scratch | — | W core.rs:295 (reset_indexes clear), match_set.rs:368,450; R match_set.rs:335,425 |
| 118 | index_icase_yes | scratch | — | W core.rs:296 (reset_indexes clear), match_set.rs:398; R match_set.rs:385 |
| 119 | index_icase_no | scratch | — | W core.rs:297 (reset_indexes clear), match_set.rs:400; R match_set.rs:383 |
| 120 | index_readingSet_yes | scratch | — | W core.rs:288–289 (clear),497–498 (set_grammar resize), match_set.rs:1221; R match_set.rs:1048 |
| 121 | index_readingSet_no | scratch | — | W core.rs:291–292 (clear),499–500 (resize), match_set.rs:1224; R match_set.rs:1045 |
| 122 | index_ruleCohort_no | scratch | — | W many `.clear(0)` in dispatch/restructure/schedule/window on restructuring, single_rule.rs:351; R single_rule.rs:348 |
| 123 | reset_cohorts_for_loop | scratch | LATCH | W restructure.rs:771,1020,1377, dispatch.rs:320,365,466 (set true on cohort-list change), single_rule.rs:838,855 (reset false); R single_rule.rs:844,861. Loop-control signal restructure→loop. |
| 124 | finish_reading_loop | scratch | LATCH | W restructure.rs:140,696, dispatch.rs:476–525 (set false), single_rule.rs:238 (reset true); R single_rule.rs:849 |
| 125 | finish_cohort_loop | scratch | LATCH | W restructure.rs:190, dispatch.rs:160 (set false), single_rule.rs:53 (reset true); R single_rule.rs:840,857 |
| 126 | in_nested | scratch | SWAPPER | W dispatch.rs:564 (set true),586 (reset false) — brackets a nested run; R single_rule.rs:85. Frame guard → stays in RuleScratch. |
| 127 | used_regex | scratch | — | W single_rule.rs:376 (reset 0 per cohort),774 (increment); R single_rule.rs:521 |
| 128 | subs_any | scratch | — | W schedule.rs:380 (push amalgam reading id),392 (clear); R schedule.rs:387. See §2 footnote c. |

### Footnotes — ambiguity resolutions (§2)

- **(a) `match_single` (74) / `match_comp` (75) / `match_sub` (76) → diag.**
  These are match-statistics counters. `match_single`/`match_sub` are write-only
  (`++` in the matcher hot path, never read); `match_comp` is fully dead (never
  even incremented). They are C++ verbose-stats counters whose shutdown reporting
  is not wired. Bucketed **diag** regardless of current deadness, because that is
  their semantic home once verbose stats are ported (`match_comp` is separately
  flagged DEAD, `match_single`/`match_sub` WRITE-ONLY in the wart inventory).
- **(b) `context_target` (96) → scratch, WRITE-ONLY (verified).** `rg -n
  "context_target"` over `crates/cg3/{src,tests}` returns exactly the ctor init
  (mod.rs:539) plus five writers (restructure.rs:59,805; single_rule.rs:545,578;
  dispatch.rs:1331) and **zero readers**. Part C left this as an unverified
  "candidate"; it is now confirmed write-only. It stays **scratch** (per-rule
  attach residue) but is added to the WRITE-ONLY wart class.
- **(c) `subs_any` (128) → scratch (doc-comment disagreement noted).** The struct
  doc-comment (mod.rs:417–419) says the amalgam readings live in the readings arena
  and "only the id is tracked here." It is cleared transiently every
  `subs_any_clear()` (schedule.rs:392) and only holds `ReadingId` handles, so by the
  clearing/transience test it is **scratch**, not doc — the ids reference doc-owned
  arena readings, but the handle list itself is per-`get_sub_reading(GSR_ANY)`
  scratch. (This overrides part C, which placed it in doc on the strength of the
  arena-backing comment.)
- **(d) `did_index` (62) → cfg.** A setup-time idempotency latch: `index()`
  self-guards (core.rs:599) so the schedule is built exactly once even when
  `run_grammar` re-calls it. It is written only during setup and never during the
  run, so it is config, not run-phase document state.
- **(e) `seen_barrier` (21) → scratch.** Written true/false around barrier tests in
  the contextual-test loop (run_contextual_test.rs:272,292; reset single_rule.rs:598,
  dispatch.rs:1315) and read within the same test dispatch (dispatch.rs:1327).
  Per-test transient → scratch.
- **(f) run-phase latches → doc.** `input_eof` (20), `dep_has_spanned` (29),
  `has_dep` (46), `dep_highest_seen` (48), `has_relations` (50) all look like
  config booleans but are mutated during the run to record what the *running
  document* acquired (EOF reached, dependency spanned a window boundary, dependency
  / relation structure seen, highest dep number seen). They are document-lifetime
  latches → **doc** (not cfg, not scratch). `did_final_enclosure` (87),
  `reset_cohorts_for_loop`/`finish_*_loop` (123–125) are also latches but
  per-window/per-frame transients → **scratch**, not doc.

## Wart inventory

Warts grouped by class, with a disposition each. "Disposition" = what
`engine-decomp.warts` should do to the field (deletion, parameterization, or a
documented keep). Bucket assignments above already reflect these.

### Class DEAD — no reads, or no uses at all

| field | # | evidence | disposition |
|-------|---|----------|-------------|
| owns_grammar | 19 | W mwesplit_applicator.rs:101; R none. core.rs:259 is only a comment describing the C++ `if (owns_grammar) delete grammar` destructor. Rust owns `Grammar` by value. | **Delete outright** in engine-decomp.warts — the type system makes it a no-op; nothing can observe it. |
| filebase | 54 | W ctor only (`None`); R none — `filebase()` (core.rs:1890) hardcodes `""`. Faithful port of C++ `nullptr`. | **Delete outright** — the accessor's constant `""` subsumes it; no reader ever sees the field. |
| match_comp | 75 | No use anywhere but field def + ctor. C++ companion counter, never incremented in the port. | **Delete outright** — carries no runtime behavior. |
| ss_taglist | 112 | W/R none in src or tests; only ctor `ScopedStack::new()`. Unexercised C++ pool. | **Delete outright** — no method in the ported call graph touches it. |

### Class WRITE-ONLY — written, never read (port gaps)

Each was verified to have zero readers. Disposition is per field: some are pure
port gaps whose deferred reader is worth wiring; others should just be deleted.

| field | # | evidence | disposition |
|-------|---|----------|-------------|
| dry_run | 18 | W set_options core.rs:1593; R none. C++ gates the actual reflow/output mutations on it. | **Operator sign-off (wire-or-delete).** Either wire the run-phase gate the C++ has, or drop `--dry-run` from the option surface and delete the field. Not a silent delete — it is a real port gap. |
| debug_level | 44 | W set_options core.rs:1634 (DODEBUG); R none. C++ prints "Debug level set to N" and gates verbose diagnostics. | **Operator sign-off (wire-or-delete).** Same shape as dry_run: wire the deferred debug diagnostics or drop `--debug`. Keep only while the option is accepted. |
| span_pattern_latin | 55 | W index core.rs:680 (setup); R none. | **Delete unless deferred reader wired.** Both span patterns exist solely for the dependency-span print path (`printSingleWindow`), which is deferred I/O. Tied to that formatter — delete now and regenerate when the formatter lands, OR keep as a pair if the formatter is imminent. Recommend delete-with-the-formatter (regenerate from `hard_limit` when needed). |
| span_pattern_utf | 56 | W index core.rs:679 (setup); R none. | Same as span_pattern_latin (they are a pair). |
| match_single | 74 | W(RUN) match_set.rs:821 (`++`); R none. C++ verbose match stats. | **Delete unless deferred reader wired.** If verbose match stats are ported, they belong in Diagnostics; otherwise delete the counter. Recommend delete now (regenerate under diag when stats land). |
| match_sub | 76 | W(RUN) match_set.rs:1188,1193 (`++`); R none. C++ verbose match stats. | Same as match_single. |
| tag_end | 81 | W(setup) set_grammar core.rs:484; R none — run-phase uses the `endtag` hash form. | **Delete.** The cached `TagId` is redundant with `endtag` (hash); every consumer routes through the hash. Regenerate from the hash if a future C++-parity path needs the id. |
| tag_subst | 82 | W(setup) set_grammar core.rs:485; R none — run-phase uses `substtag` hash. | Same as tag_end. |
| context_target | 96 | W ×5 (restructure.rs:59,805; single_rule.rs:545,578; dispatch.rs:1331); R none (verified). C++ mirror read by attach diagnostics/tracing not yet ported. | **Delete unless the deferred attach-diagnostics reader is worth wiring.** Verified write-only here (part C was unsure). Recommend delete now — it is pure write residue in the current call graph — and regenerate if/when attach diagnostics are ported. |

### Class SWAPPER — save/mutate/restore of a field that should be a parameter or frame state

| field | # | pattern | disposition |
|-------|---|---------|-------------|
| trace | 7 | C++ `swapper<bool>(true, trace, ttrace)` in `print_debug_rule` (core.rs:1737 save → 1738 false → 1778 restore) and `add_profiling_example` (core.rs:1791/1792/1809). Load-bearing: the printers read `self.trace` transitively through `print_single_window`, so the temporary `trace=false` observably suppresses per-reading trace tags in the buffered snapshot. | **The swap must become a parameter** so `trace` can live in **cfg**. Thread a `trace: bool` (or a "suppress trace" flag) down the `print_single_window` path used by the two profiling/debug printers, instead of mutating the shared config field. `trace` itself is not run-phase mutable state. |
| tmpl_cntx | 90 | Save/restore of `min/max/in_template` around recursive template descent (run_contextual_test.rs:449–513; match_set.rs:1252–1300) + hard-reset at rule/frame boundaries. | **Load-bearing frame state → RuleScratch; MAY stay save/restore.** It is genuine per-frame template-matching state; keep the save/restore idiom inside RuleScratch. |
| current_rule | 98 | Save/restore around a nested subrule run (dispatch.rs:567 save → 572 set → 584 restore) + set per scheduled rule (schedule.rs:75). | **Load-bearing frame state → RuleScratch; MAY stay save/restore.** Names the rule currently applying; the swap is the nested-run guard. |
| in_nested | 126 | Set true at dispatch.rs:564, false at 586, bracketing a nested `run`; read single_rule.rs:85 to suppress the outer-frame cohortset push. | **Load-bearing frame state → RuleScratch; MAY stay save/restore** (it toggles to a fixed `false` rather than saving a prior value, but semantically it is the same bracket). |

Note the asymmetry: `trace` lives in **cfg**, so its swap *cannot* stay — it must
be parameterized. The other three are frame state that moves to RuleScratch and may
keep the save/restore shape there.

### Class LATCH — config-looking but legitimately run-mutated

Not warts to remove — only bucket corrections (these must NOT go in cfg).

| field | # | disposition |
|-------|---|-------------|
| input_eof | 20 | **doc** — run-phase EOF latch. |
| dep_has_spanned | 29 | **doc** — one-shot latch when a dependency crosses a window boundary. |
| has_dep | 46 | **doc** — document acquired dependency structure (has a setup writer too, but the run-phase writes are decisive). |
| dep_highest_seen | 48 | **doc** — highest dep number seen so far (per-window reset). |
| has_relations | 50 | **doc** — document acquired relation structure (no setup writer at all). |
| did_final_enclosure | 87 | **scratch** — per-window enclosure phase toggle. |
| reset_cohorts_for_loop / finish_reading_loop / finish_cohort_loop | 123–125 | **scratch** — per-frame loop-control signals. |

### Class MRU — config content, cache mutation at run time

| field | # | evidence | disposition |
|-------|---|----------|-------------|
| text_delimiters | 103 | Compiled in set_grammar (core.rs:526) / set_text_delimiter (core.rs:551,578); reordered at run time by `test_string_against`'s `rxs.swap(0,i)` move-to-front (called run_grammar.rs:1104 with `&mut self.text_delimiters`). The reorder is a pure MRU optimization with no observable output effect — faithful port of the C++ `std::rotate`/swap in `isText`. | **Keep MRU (C++ behavior).** The *content* is cfg; the field just needs its `&mut` reachable at the delimiter-test call site. Two options: (i) live in scratch-adjacent storage that owns the mutable cache; or (ii) stay in cfg with interior mutability / accept `&mut cfg` during the stream read. **Recommendation: keep the storage semantically cfg but hold it behind a small owned MRU cache reachable by `&mut` on the stream-read path** (i.e. option (i) — scratch-adjacent placement of the mutable vector, config-derived content). This keeps `EngineConfig` conceptually read-only during the run without forcing interior mutability into the config view. |

## Spec-remap plan

**Anchor rules.** The struct is anchored by the single def rule
`[spec:cg3:def:grammar-applicator.cg3.grammar-applicator]`
(docs/spec/port/src/GrammarApplicator.md:15–139) — the verbatim C++ `class
GrammarApplicator { ... }` field list. All method behavior hangs off ~99 sibling
`[spec:cg3:def/sem:grammar-applicator.cg3.grammar-applicator.<fn>]` rules in the
same file (and the `_matchSet` / `_context` / `_reflow` / `_runContextualTest` /
`_runGrammar` / `_runRules` companion files, which anchor method groups but not the
struct itself). Spec rules are markdown-anchored blocks with no inline version
field; versioning is carried at the WBS/commit level by the nplan/nspec system, so a
"version bump" here means: at each rehome commit, redefine the affected def rule and
let nplan record the bump against `engine-decomp.*`.

**Plan.**

1. **Per-bucket def redefinitions, one per rehome commit.** The single
   `grammar-applicator` struct def is split, in place, into the four view structs as
   each bucket is rehomed. Concretely: when the cfg fields move to `EngineConfig`,
   redefine the struct def to show `EngineConfig { ... }` holding exactly the cfg-bucket
   fields (with a bump recorded at the cfg rehome commit); likewise for `Document`
   (doc), `RuleScratch` (scratch), `Diagnostics` (diag). Each rehome is its own
   commit with its own def redefinition + bump — do NOT rewrite the whole struct in
   one shot. The `grammar` field and the predecided `window`/`store`/`profiler`
   placements are reflected in whichever view owns them (grammar stays free-standing;
   window/store → Document; profiler → Diagnostics).

2. **`sem` rules stay on their functions.** The method `sem` rules describe behavior,
   not layout; they do NOT move buckets. They only need touching where a field they
   name is deleted or renamed (see 3). The function inventory stays 1:1, so no `sem`
   rule is added or removed — the `Engine<'_>` split-borrow view (see Decision
   record) lets each ported method keep its identity while reading its fields through
   whichever view now owns them.

3. **Wart deletions need errata/bumps on any `sem` rule that pins the deleted
   behavior.** Deleting a field means the `sem` rules that mention it must get an
   errata note + bump at the deletion commit:
   - **trace-swap** — `[spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]`
     (GrammarApplicator.md:921, "Constructs a scoped `swapper<bool>(true, trace,
     ttrace)`…") and
     `[spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]`
     (GrammarApplicator.md:144, "Construct a scoped `swapper<bool>(true, trace,
     ttrace)`…"). When the trace swap becomes a parameter, both need an errata note
     that the `swapper<bool>` idiom is replaced by threading a `trace`/suppress-trace
     parameter (behavior identical; mechanism changed) + a bump. These two are the
     only `sem` rules that mention the trace swap.
   - **profiling-example / match counters** — the `add-profiling-example-fn` sem also
     backs `match_single`/`match_sub`/`match_comp` context indirectly only via the
     matcher sems; `match_single`'s increment is pinned by
     `[spec:cg3:sem:...does-tag-match-reading-fn]` ("on a match increments
     `match_single`", GrammarApplicator.md:467/501) and `match_sub` by
     `[spec:cg3:sem:...does-set-match-reading-fn]` ("++match_sub",
     GrammarApplicator.md:411). If those counters are deleted, those two sems get an
     errata note (counter removed — no observable effect) + bump. `match_comp` is
     mentioned by no sem rule, so its deletion needs only the struct-def bump.
   - **owns_grammar** — pinned by
     `[spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.grammar-applicator-fn]`
     (destructor, GrammarApplicator.md:673, "If `owns_grammar` is true, `delete
     grammar`"). Deleting the field needs an errata on that destructor sem (Rust owns
     grammar by value; the branch is vacuous) + bump.
   - **filebase, tag_end, tag_subst, span_pattern_*, context_target, ss_taglist** —
     none are pinned by a `sem` rule beyond the struct def (filebase's accessor
     already returns `""`; the span patterns' consumer sem is unported; tag_end/
     tag_subst have no reader sem; ss_taglist is unexercised). Their deletions need
     only the per-bucket struct-def bump, no `sem` errata.

## Decision record

1. **Four buckets.** Fields are partitioned into `EngineConfig` (cfg, setup-only /
   run-read-only), `Document` (doc, run-mutable document-lifetime state),
   `RuleScratch` (scratch, per-rule/window/cohort transient), and `Diagnostics`
   (diag, profiler/trace/stats). Every field lands in exactly one.

2. **Predecided fields honored.** `grammar` stays its own field; `window`
   (C++ `gWindow`) and `store` → doc; `profiler` → diag. Not re-litigated.

3. **Function inventory stays 1:1.** No ported function is split, merged, added, or
   removed by this decomposition. The buckets are a *view* over the same state, not a
   behavioral refactor.

4. **`Engine<'_>` split-borrow view.** The four view structs are recombined behind an
   `Engine<'_>` (or equivalent) that hands each method simultaneous `&`/`&mut` access
   to the distinct views it needs, so the 1:1 methods keep their signatures and their
   field access while the storage is split. This is the mechanism that lets cfg stay
   read-only during the run while scratch/doc mutate.

5. **DEAD warts deleted outright.** `owns_grammar` (19), `filebase` (54),
   `match_comp` (75), `ss_taglist` (112) have no live reads and no observable effect;
   they are deleted in `engine-decomp.warts` (not relocated), with struct-def bumps
   and the one destructor-sem errata for `owns_grammar`.

6. **WRITE-ONLY warts: delete-by-default, two need operator sign-off.**
   `span_pattern_latin`/`span_pattern_utf` (55/56), `match_single`/`match_sub`
   (74/76), `tag_end`/`tag_subst` (81/82), and `context_target` (96) are deleted now
   and regenerated if/when their deferred consumers are ported. **`dry_run` (18) and
   `debug_level` (44) require operator sign-off**: they are option-surface port gaps —
   wire the deferred C++ gate (dry-run mutation gate; debug diagnostics) OR drop the
   `--dry-run` / `--debug` option and delete the field. Not a silent decision.

7. **SWAPPER warts split by home.** `trace` (7) lives in cfg, so its
   `swapper<bool>` save/restore in `print_debug_rule` / `add_profiling_example` MUST
   become a threaded parameter (with sem errata on both fns). `tmpl_cntx` (90),
   `current_rule` (98), `in_nested` (126) are load-bearing frame state → RuleScratch,
   and MAY keep the save/restore idiom there.

8. **LATCH fields are bucket fixes, not deletions.** `input_eof` (20),
   `dep_has_spanned` (29), `has_dep` (46), `dep_highest_seen` (48), `has_relations`
   (50) → doc; `did_final_enclosure` (87), `reset_cohorts_for_loop`/
   `finish_reading_loop`/`finish_cohort_loop` (123–125) → scratch. They stay,
   correctly bucketed.

9. **text_delimiters keeps its MRU behavior** (Decision: scratch-adjacent mutable
   cache holding config-derived content, reachable by `&mut` on the stream-read path;
   `EngineConfig` stays conceptually read-only during the run). Behavior preserved.

10. **§2 ambiguity footnotes are binding.** match counters → diag (a);
    `context_target` verified write-only, scratch bucket (b); `subs_any` → scratch
    despite its arena-backing doc-comment (c); `did_index` → cfg (d);
    `seen_barrier` → scratch (e); the five run-phase latches → doc (f).

## Open questions

1. **dry_run / debug_level wire-vs-delete** — genuinely unresolved; needs the port
   owner's call on whether the deferred C++ behavior (dry-run output gate; debug
   diagnostics) is in scope for this port at all. Until then the `--dry-run` /
   `--debug` options accept a value that does nothing.

2. **Which deferred formatters are imminent** — `span_pattern_latin/utf` (dependency
   -span print) and `match_single/sub` (verbose stats) are recommended for deletion
   now, but if the dependency-span formatter or verbose-stats reporting is scheduled
   soon, keeping them (in cfg / diag respectively) avoids a regenerate churn. Depends
   on the WBS ordering after `engine-decomp.warts`.

3. **text_delimiters placement mechanics** — the recommendation (scratch-adjacent
   owned MRU cache) is chosen, but the exact home of that cache within the
   `Engine<'_>` split-borrow view (does it hang off Document, off a dedicated
   run-cache view, or interior-mutable on cfg) is an implementation detail to settle
   when the borrow structure is built.

4. **`context_target`'s C++ consumer** — deletion assumes the C++ reader is attach
   diagnostics/tracing that is simply unported. If a *behavioral* (non-diagnostic)
   C++ consumer exists that the port silently dropped, deletion would mask a port
   gap. Low risk (five writers, all attach-adjacent), but worth a C++-side confirm
   before the deletion commit.
