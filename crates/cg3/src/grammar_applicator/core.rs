//! `src/GrammarApplicator.cpp` (+ the inline getters/setters/printers of
//! `src/GrammarApplicator.hpp`) — the engine construction / indexing / tag
//! interning / stream printing / EXTERNAL-pipe I/O method bodies.
//!
//! ARENA + STORE THREADING (important port note).
//! The task's ownership model says the applicator OWNS `self.doc.store:
//! RuntimeStore` (the pooled `Cohort`/`Reading`/`SingleWindow` arenas), and the
//! C++ `Cohort*`/`Reading*`/`SingleWindow*` are resolved through it. **That field
//! is currently MISSING from `grammar_applicator/mod.rs`** (the scaffold only has
//! `gWindow: Window`, and a `Window` does not own the arenas — see
//! `window.rs`/`store.rs`). Since this pass may edit ONLY `core.rs`, every method
//! that must resolve a runtime id takes an explicit `store: &mut RuntimeStore` /
//! `&RuntimeStore` parameter, matching the free-fn convention already used by
//! `cohort.rs`/`reading.rs`/`window.rs`/`single_window.rs`. When `mod.rs` gains a
//! `pub store: RuntimeStore` field these params should collapse into
//! `let GrammarApplicator { grammar, store, gWindow, .. } = self;` destructuring.
//!
//! OUTPUT SINK. The C++ `std::ostream& output` becomes a generic
//! `output: &mut W` (`W: std::io::Write`); the ported `uextras::{u_fprintf,
//! u_fputc, u_fflush}` primitives write UTF-8 to it. `%S`/`%u` printf tokens are
//! translated to Rust `format_args!` interpolation. The EXTERNAL `Process&`
//! endpoints are bridged with the local [`ProcWrite`]/[`ProcRead`] adapters.
//!
//! PLACEHOLDERS. `self.diag.profiler`/`self.ux_stderr`/`self.ux_stdin`/`self.ux_stdout`
//! are `Option<()>` stand-ins (no Profiler module, no wired streams yet), so the
//! `error(...)`/`printDebugRule`/`addProfilingExample`/`profileRuleContext`
//! emissions are built faithfully but not flushed to a real stream (noted inline).

use std::io::{Read, Write};

use regex::RegexBuilder;

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, SwId, TagId};
use crate::cohort::{CT_RELATED, CT_REMOVED, DEP_NO_PARENT, unignore_all};
use crate::contextual_test::POS_NEGATE;
use crate::grammar::Grammar;
use crate::inlines::{
    cg3_quit, g_app_set_opts_ranged, hash_value_ustring, is_textual, isnl, read_raw, read_utf8_raw,
    ui8, ui32, write_raw, write_utf8_raw,
};
use crate::options::{OPTIONS, options_t};
use crate::process::Process;
use crate::reading::Reading;
use crate::store::RuntimeStore;
use crate::strings::KEYWORDS;
use crate::tag::{
    T_CASE_INSENSITIVE, T_DEPENDENCY, T_MAPPING, T_PRESERVE_ESC, T_REGEXP, T_RELATION, T_TEXTUAL,
    T_VARSTRING, Tag,
};
use crate::tag_trie::trie_get_tag_list_append;
use crate::types::{GlobalNumber, TagHash};
use crate::uextras::{u_fflush, u_fputc, ux_strCaseCompare};

use super::{Engine, tmpl_context_t};

// ===========================================================================
// Local port infrastructure.
//
// These consts/fns belong in `strings.rs` (the `STR_*` command literals + the
// `keywords[]` name table) / `streambuf.rs` (the `Process` <-> `io` bridge) but
// are not yet ported there (`Strings.md` explicitly scopes out the name table).
// They are defined here — un-annotated, port-infra — so `core.rs` compiles
// standalone; a later consolidation pass should relocate them.
// ===========================================================================

// C++ `Strings.hpp` string constants (UTF-16 → UTF-8 &str).
const STR_BEGINTAG: &str = ">>>";
const STR_ENDTAG: &str = "<<<";
const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";
const STR_CMD_SETVAR: &str = "<STREAMCMD:SETVAR:";
const STR_CMD_REMVAR: &str = "<STREAMCMD:REMVAR:";
const STR_CMD_FLUSH: &str = "<STREAMCMD:FLUSH>";
const STR_TEXTDELIM_DEFAULT: &str = "/(^|\\n)</s/r";

/// C++ `Strings.hpp` `constexpr UStringView keywords[KEYWORD_COUNT]` — the
/// keyword name table indexed by [`KEYWORDS`]. Not ported in `strings.rs`
/// (out of that module's spec scope), reproduced here verbatim for `print_trace`.
fn keyword_name(k: KEYWORDS) -> &'static str {
    use KEYWORDS::*;
    match k {
        K_IGNORE => "__CG3_DUMMY_KEYWORD__",
        K_SETS => "SETS",
        K_LIST => "LIST",
        K_SET => "SET",
        K_DELIMITERS => "DELIMITERS",
        K_SOFT_DELIMITERS => "SOFT-DELIMITERS",
        K_PREFERRED_TARGETS => "PREFERRED-TARGETS",
        K_MAPPING_PREFIX => "MAPPING-PREFIX",
        K_MAPPINGS => "MAPPINGS",
        K_CONSTRAINTS => "CONSTRAINTS",
        K_CORRECTIONS => "CORRECTIONS",
        K_SECTION => "SECTION",
        K_BEFORE_SECTIONS => "BEFORE-SECTIONS",
        K_AFTER_SECTIONS => "AFTER-SECTIONS",
        K_NULL_SECTION => "NULL-SECTION",
        K_ADD => "ADD",
        K_MAP => "MAP",
        K_REPLACE => "REPLACE",
        K_SELECT => "SELECT",
        K_REMOVE => "REMOVE",
        K_IFF => "IFF",
        K_APPEND => "APPEND",
        K_SUBSTITUTE => "SUBSTITUTE",
        K_START => "START",
        K_END => "END",
        K_ANCHOR => "ANCHOR",
        K_EXECUTE => "EXECUTE",
        K_JUMP => "JUMP",
        K_REMVARIABLE => "REMVARIABLE",
        K_SETVARIABLE => "SETVARIABLE",
        K_DELIMIT => "DELIMIT",
        K_MATCH => "MATCH",
        K_SETPARENT => "SETPARENT",
        K_SETCHILD => "SETCHILD",
        K_ADDRELATION => "ADDRELATION",
        K_SETRELATION => "SETRELATION",
        K_REMRELATION => "REMRELATION",
        K_ADDRELATIONS => "ADDRELATIONS",
        K_SETRELATIONS => "SETRELATIONS",
        K_REMRELATIONS => "REMRELATIONS",
        K_TEMPLATE => "TEMPLATE",
        K_MOVE => "MOVE",
        K_MOVE_AFTER => "MOVE-AFTER",
        K_MOVE_BEFORE => "MOVE-BEFORE",
        K_SWITCH => "SWITCH",
        K_REMCOHORT => "REMCOHORT",
        K_STATIC_SETS => "STATIC-SETS",
        K_UNMAP => "UNMAP",
        K_COPY => "COPY",
        K_ADDCOHORT => "ADDCOHORT",
        K_ADDCOHORT_AFTER => "ADDCOHORT-AFTER",
        K_ADDCOHORT_BEFORE => "ADDCOHORT-BEFORE",
        K_EXTERNAL => "EXTERNAL",
        K_EXTERNAL_ONCE => "EXTERNAL-ONCE",
        K_EXTERNAL_ALWAYS => "EXTERNAL-ALWAYS",
        K_OPTIONS => "OPTIONS",
        K_STRICT_TAGS => "STRICT-TAGS",
        K_REOPEN_MAPPINGS => "REOPEN-MAPPINGS",
        K_SUBREADINGS => "SUBREADINGS",
        K_SPLITCOHORT => "SPLITCOHORT",
        K_PROTECT => "PROTECT",
        K_UNPROTECT => "UNPROTECT",
        K_MERGECOHORTS => "MERGECOHORTS",
        K_RESTORE => "RESTORE",
        K_WITH => "WITH",
        K_OLIST => "OLIST",
        K_OSET => "OSET",
        K_CMDARGS => "CMDARGS",
        K_CMDARGS_OVERRIDE => "CMDARGS-OVERRIDE",
        K_COPYCOHORT => "COPYCOHORT",
        K_REMPARENT => "REMPARENT",
        K_SWITCHPARENT => "SWITCHPARENT",
        KEYWORD_COUNT => "",
    }
}

/// C++ `r->maplist->getNonEmpty().begin()->first->tag` — the first tag text
/// of a rule's map/sub set (its `trie`, or `trie_special` when the trie is
/// empty). `getNonEmpty` is not yet ported in `set.rs`; reproduced inline.
///
/// A free `&Grammar` reader (rather than an `Engine`/`&self` method) so it is
/// callable from both the peeled `Engine` printers and the `&self` per-format
/// print paths (`print_trace`).
fn first_maplist_tag(grammar: &Grammar, set: Option<crate::arena::SetId>) -> Option<&str> {
    let sid = set?;
    let s = &grammar.sets_list[sid.0];
    let trie = if !s.trie.is_empty() {
        &s.trie
    } else {
        &s.trie_special
    };
    let (tid, _node) = trie.iter().next()?;
    Some(&grammar.single_tags_list[tid.0].tag)
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
/// C++ `void printTrace(std::ostream& output, uint32_t hit_by)`.
///
/// A free `&Grammar`/`&EngineConfig` reader so it is callable from both the
/// peeled `Engine` printers ([`Engine::print_trace`]) and the `&self`
/// per-format print paths (Apertium/Niceline `printReading`), which cannot
/// split `&self` into an `Engine` view. Reads only `grammar`/`cfg`.
pub fn print_trace<W: Write>(
    grammar: &Grammar,
    cfg: &super::EngineConfig,
    output: &mut W,
    hit_by: u32,
) {
    if (hit_by as usize) < grammar.rule_by_number.capacity() as usize
        && grammar.rule_by_number.try_get(hit_by).is_some()
    {
        let r = &grammar.rule_by_number[hit_by];
        let _ = write!(output, "{}", keyword_name(r.r#type));
        use KEYWORDS::*;
        let is_rel = matches!(
            r.r#type,
            K_ADDRELATION
                | K_SETRELATION
                | K_REMRELATION
                | K_ADDRELATIONS
                | K_SETRELATIONS
                | K_REMRELATIONS
        );
        if is_rel {
            if let Some(txt) = first_maplist_tag(grammar, r.maplist) {
                let _ = write!(output, "({txt}");
            }
            if matches!(r.r#type, K_ADDRELATIONS | K_SETRELATIONS | K_REMRELATIONS)
                && let Some(txt) = first_maplist_tag(grammar, r.sublist)
            {
                let _ = write!(output, ",{txt}");
            }
            let _ = write!(output, ")");
        }
        if !cfg.trace_name_only || r.name.is_empty() {
            let _ = write!(output, ":{}", r.line);
        }
        if !r.name.is_empty() {
            u_fputc(':', output);
            let _ = write!(output, "{}", r.name);
        }
    } else {
        // C++ ENCL pass number: numeric_limits<uint32_t>::max() - hit_by.
        let pass = u32::MAX - hit_by;
        let _ = write!(output, "ENCL:{pass}");
    }
}

/// `std::ostream` <-> `Process` write bridge (the C++ `Process& output` is
/// written with the same `writeRaw`/`output.write(...)` primitives as a stream).
struct ProcWrite<'a>(&'a mut Process);
impl Write for ProcWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.write(buf, buf.len()) {
            Ok(()) => Ok(buf.len()),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush();
        Ok(())
    }
}

/// `std::istream` <-> `Process` read bridge — each `read` fills the whole buffer
/// (the C++ `input.read(&buf[0], cs)` is an all-or-error read).
struct ProcRead<'a>(&'a mut Process);
impl Read for ProcRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len();
        match self.0.read(buf, n) {
            Ok(()) => Ok(n),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }
}

/// C++ `grammar->single_tags[hash]` (operator[]): resolve a hash to its `TagId`.
/// operator[] default-inserts a null `Tag*` on a miss (→ deref crash); here a
/// miss returns `TagId(0)` (the first tag), which cannot crash — a benign
/// divergence for the always-present hashes these call sites pass.
pub(super) fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags.find(hash.get());
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

/// C `std::stoul` leading-decimal parse (option values are always well-formed).
fn stoul(s: &str) -> u32 {
    let mut v: u64 = 0;
    for c in s.trim_start().chars() {
        match c.to_digit(10) {
            Some(d) => v = v.wrapping_mul(10).wrapping_add(d as u64),
            None => break,
        }
    }
    v as u32
}

/// C `std::stoi` leading-decimal parse with optional sign.
fn stoi(s: &str) -> i32 {
    let s = s.trim_start();
    let mut it = s.chars().peekable();
    let mut neg = false;
    if let Some(&c) = it.peek()
        && (c == '+' || c == '-')
    {
        neg = c == '-';
        it.next();
    }
    let mut v: i64 = 0;
    for c in it {
        match c.to_digit(10) {
            Some(d) => v = v.wrapping_mul(10).wrapping_add(d as i64),
            None => break,
        }
    }
    if neg { -v as i32 } else { v as i32 }
}

// ===========================================================================
// tmpl_context_t::clear
// ===========================================================================

impl tmpl_context_t {
    // [spec:cg3:def:grammar-applicator.cg3.tmpl-context-t.clear-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.tmpl-context-t.clear-fn]
    /// C++ `void tmpl_context_t::clear()` — blanks the template-test window.
    pub fn clear(&mut self) {
        self.min = None;
        self.max = None;
        self.linked.clear();
        self.in_template = false;
    }
}

// ===========================================================================
// ~GrammarApplicator (destructor)
// ===========================================================================

impl Drop for super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.grammar-applicator-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.grammar-applicator-fn]
    /// C++ `~GrammarApplicator()`. In the port every clause is subsumed by Rust
    /// ownership: `if (owns_grammar) delete grammar` + `grammar = nullptr` →
    /// `self.grammar` is owned by value and dropped here; `ux_stderr = nullptr`
    /// → placeholder; `for (rx : text_delimiters) uregex_close(rx)` → each
    /// `regex::Regex` releases on drop. Net effect: nothing to do explicitly.
    fn drop(&mut self) {}
}

impl super::GrammarApplicator {
    // =======================================================================
    // Trivial accessors (inline getters/setters from the .hpp)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-grammar-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-grammar-fn]
    /// C++ `Grammar* get_grammar()` — the attached grammar. Owned by value here,
    /// so the `Grammar*` accessor returns a borrow.
    pub fn get_grammar(&self) -> &Grammar {
        &self.grammar
    }

    // =======================================================================
    // resetIndexes
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reset-indexes-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reset-indexes-fn]
    /// C++ `void resetIndexes()` — clears the per-reading/regex/icase match
    /// caches (the per-set `yes`/`no` vectors keep their length).
    pub fn reset_indexes(&mut self) {
        for sv in &mut self.scratch.index_readingSet_yes {
            sv.clear(0);
        }
        for sv in &mut self.scratch.index_readingSet_no {
            sv.clear(0);
        }
        self.scratch.index_regexp_yes.clear(0);
        self.scratch.index_regexp_no.clear(0);
        self.scratch.index_icase_yes.clear(0);
        self.scratch.index_icase_no.clear(0);
    }

    // =======================================================================
    // addTag (Tag* internal overload + UChar*/type public overload)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
    /// C++ `Tag* addTag(const UChar* txt, uint32_t type)` — interns a tag from
    /// text and returns its canonical `TagId`. Collapses the three C++ overloads
    /// (`const UChar*` / `const UString&` / `UStringView`), which all map onto
    /// `&str`. Returns `TagId`.
    ///
    /// Public-API entry retained on `GrammarApplicator` (the format applicators,
    /// setup, and tests call it through `self.base`); the body lives on
    /// [`Engine`](super::Engine) because it is reached from the peeled contextual
    /// matcher knot (`generate_varstring_tag` → `add_tag`). This one-line
    /// split-borrow forwarder is the Stage-C boundary between the two callers.
    pub fn add_tag(&mut self, txt: &str, r#type: crate::tag::TagType) -> TagId {
        self.engine().add_tag(txt, r#type)
    }

    // =======================================================================
    // setGrammar
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-grammar-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-grammar-fn]
    /// C++ `void setGrammar(Grammar* res)` — attaches the grammar and seeds the
    /// begin/end/subst/mprefix tags, the per-set match caches, and compiles the
    /// grammar's own text-delimiter regexes.
    ///
    /// DIVERGENCE: the C++ takes `Grammar* res` and assigns `grammar = res`;
    /// here the grammar is owned at construction (`new(grammar)`), so this
    /// operates on `self.grammar` and takes no argument.
    pub fn set_grammar(&mut self) -> Result<(), crate::error::Cg3Error> {
        let tb = self.add_tag(STR_BEGINTAG, crate::tag::TagType::empty());
        let te = self.add_tag(STR_ENDTAG, crate::tag::TagType::empty());
        let ts = self.add_tag(STR_DUMMY, crate::tag::TagType::empty());
        self.cfg.tag_begin = Some(tb);
        self.cfg.begintag = self.grammar.single_tags_list[tb.0].hash;
        self.cfg.endtag = self.grammar.single_tags_list[te.0].hash;
        self.cfg.substtag = self.grammar.single_tags_list[ts.0].hash;

        let mp: String = self.grammar.mapping_prefix.to_string();
        let k = self.add_tag("_MPREFIX", crate::tag::TagType::empty());
        self.cfg.mprefix_key = self.grammar.single_tags_list[k.0].hash;
        let v = self.add_tag(&mp, crate::tag::TagType::empty());
        self.cfg.mprefix_value = self.grammar.single_tags_list[v.0].hash;

        let n = self.grammar.sets_list.capacity() as usize;
        self.scratch.index_readingSet_yes.clear();
        self.scratch
            .index_readingSet_yes
            .resize_with(n, Default::default);
        self.scratch.index_readingSet_no.clear();
        self.scratch
            .index_readingSet_no
            .resize_with(n, Default::default);

        if let Some(td_set) = self.grammar.text_delimiters {
            // Flatten the delimiter set's tries (both immutable borrows of grammar).
            let mut the_tags: Vec<TagId> = Vec::new();
            trie_get_tag_list_append(
                &self.grammar.sets_list[td_set.0].trie,
                &mut the_tags,
                &self.grammar,
            );
            trie_get_tag_list_append(
                &self.grammar.sets_list[td_set.0].trie_special,
                &mut the_tags,
                &self.grammar,
            );
            // Collect (pattern, icase) so the grammar borrow ends before we push
            // into self.text_delimiters.
            let specs: Vec<(String, bool)> = the_tags
                .iter()
                .map(|t| {
                    let tag = &self.grammar.single_tags_list[t.0];
                    (tag.tag.clone(), tag.r#type.intersects(T_CASE_INSENSITIVE))
                })
                .collect();
            for (pat, icase) in specs {
                match RegexBuilder::new(&pat).case_insensitive(icase).build() {
                    Ok(re) => self.cfg.text_delimiters.push(re),
                    Err(_) => {
                        // "Error: uregex_open returned ... - cannot continue!"
                        crate::error::emit_cg3quit_line(file!(), self.doc.num_lines);
                        return Err(crate::error::Cg3Error::fatal(1, None));
                    }
                }
            }
        }
        Ok(())
    }

    // =======================================================================
    // setTextDelimiter
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
    /// C++ `void setTextDelimiter(UString rx)` — replaces the compiled
    /// text-delimiter regex from a (possibly `/.../ri`-wrapped) pattern.
    pub fn set_text_delimiter(
        &mut self,
        rx: crate::types::UString,
    ) -> Result<(), crate::error::Cg3Error> {
        // uregex_close(r) for each: regex::Regex drops here.
        self.cfg.text_delimiters.clear();

        if rx.is_empty() {
            return Ok(());
        }

        let mut chars: Vec<char> = rx.chars().collect();
        let mut icase = false;
        if chars.len() >= 3 && chars[0] == '/' {
            chars.remove(0);
            while let Some(&back) = chars.last() {
                if back == '/' || back == 'r' || back == 'i' {
                    if back == 'i' {
                        icase = true;
                    } else if back == '/' {
                        chars.pop();
                        break;
                    }
                    chars.pop();
                } else {
                    break;
                }
            }
        }

        let pat: String = chars.into_iter().collect();
        match RegexBuilder::new(&pat).case_insensitive(icase).build() {
            Ok(re) => self.cfg.text_delimiters.push(re),
            Err(_) => {
                crate::error::emit_cg3quit_line(file!(), self.doc.num_lines);
                return Err(crate::error::Cg3Error::fatal(1, None));
            }
        }
        Ok(())
    }

    // =======================================================================
    // index
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.index-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.index-fn]
    /// C++ `void index()` — builds the per-section rule schedule and the
    /// dependency span-print patterns; runs at most once.
    pub fn index(&mut self) {
        if !self.cfg.add_spacing {
            self.cfg.ws[2] = '\n';
        }
        if self.cfg.did_index {
            return;
        }

        if self.grammar.ordered {
            self.cfg.ordered = true;
        }
        if self.grammar.has_dep || self.cfg.dep_delimit != 0 {
            self.cfg.parse_dep = true;
        }

        if !self.grammar.before_sections.is_empty() {
            let rules: Vec<RuleId> = self.grammar.before_sections.clone();
            let m = self.cfg.runsections.entry(-1).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }
        if !self.grammar.after_sections.is_empty() {
            let rules: Vec<RuleId> = self.grammar.after_sections.clone();
            let m = self.cfg.runsections.entry(-2).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }
        if !self.grammar.null_section.is_empty() {
            let rules: Vec<RuleId> = self.grammar.null_section.clone();
            let m = self.cfg.runsections.entry(-3).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }

        if self.cfg.sections.is_empty() {
            let smax = self.grammar.sections.len() as i32;
            let rules: Vec<RuleId> = self.grammar.rules.clone();
            for i in 0..smax {
                for &r in &rules {
                    let rule = &self.grammar.rule_by_number[r.0];
                    if rule.section < 0 || rule.section > i {
                        continue;
                    }
                    let num = rule.number;
                    self.cfg.runsections.entry(i).or_default().insert(num);
                }
            }
        } else {
            self.cfg.numsections = ui32(self.cfg.sections.len());
            let rules: Vec<RuleId> = self.grammar.rules.clone();
            let sections = self.cfg.sections.clone();
            for n in 0..self.cfg.numsections {
                for e in 0..=n {
                    for &r in &rules {
                        let rule = &self.grammar.rule_by_number[r.0];
                        if rule.section != (sections[e as usize] as i32) - 1 {
                            continue;
                        }
                        let num = rule.number;
                        self.cfg
                            .runsections
                            .entry(n as i32)
                            .or_default()
                            .insert(num);
                    }
                }
            }
        }

        if !self.cfg.valid_rules.empty() {
            let mut vr = crate::interval_vector::uint32IntervalVector::new();
            for i in 0..self.grammar.rule_by_number.capacity() {
                if let Some(rule) = self.grammar.rule_by_number.try_get(i)
                    && self.cfg.valid_rules.contains(rule.line)
                {
                    vr.insert_sorted(rule.number);
                }
            }
            self.cfg.valid_rules = vr;
        }

        // Dependency span print patterns (state kept for parity; print_reading
        // reproduces the format directly via dep_span_width()).
        let w = ui8((self.cfg.hard_limit as f64).log10().floor() + 1.0);
        let wc = char::from_digit(w as u32, 10).unwrap_or('1');
        self.cfg.span_pattern_utf = format!(" #%u%0{wc}u\u{2192}%u%0{wc}u");
        self.cfg.span_pattern_latin = format!(" #%u%0{wc}u->%u%0{wc}u");

        self.cfg.did_index = true;
    }

}

impl Engine<'_> {
    /// Dependency span zero-pad width — `floor(log10(hard_limit)) + 1`, matching
    /// the digit baked into `span_pattern_*` by [`index`].
    fn dep_span_width(&self) -> usize {
        (ui8((self.cfg.hard_limit as f64).log10().floor() + 1.0) as usize).max(1)
    }

    // =======================================================================
    // Stream printing (ostream& → &mut W: Write)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-stream-command-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-stream-command-fn]
    /// C++ `void printStreamCommand(UStringView cmd, std::ostream& output)`.
    ///
    /// (The C++ virtual dispatch to per-format overrides is the
    /// [`StreamFormat`](super::stream_format::StreamFormat) strategy; this is
    /// the base-class implementation.)
    pub fn print_stream_command<W: Write>(&self, cmd: &str, output: &mut W) {
        let _ = writeln!(output, "{cmd}");
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-plain-text-line-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-plain-text-line-fn]
    /// C++ `void printPlainTextLine(UStringView line, std::ostream& output)`.
    ///
    /// (The C++ virtual dispatch to per-format overrides is the
    /// [`StreamFormat`](super::stream_format::StreamFormat) strategy; this is
    /// the base-class implementation.)
    pub fn print_plain_text_line<W: Write>(&self, line: &str, output: &mut W) {
        let _ = write!(output, "{line}");
    }

    /// C++ `void printTrace(std::ostream& output, uint32_t hit_by)` — thin `&self`
    /// wrapper delegating to the free [`print_trace`] (which reads only
    /// `grammar`/`cfg`, so it is also callable from `&self` non-Engine printers).
    pub fn print_trace<W: Write>(&self, output: &mut W, hit_by: u32) {
        print_trace(self.grammar, self.cfg, output, hit_by);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    /// C++ `void printReading(const Reading* reading, std::ostream& output,
    /// size_t sub = 1)`. Resolves `Reading*`/`Cohort*` through `store`.
    ///
    /// `trace` is the effective trace flag threaded down the print chain
    /// (normally `self.trace`; the profiling printers pass `false`). The C++
    /// read `this->trace` directly and its two profiling callers force-disabled
    /// it via a `swapper<bool>`; threading the value keeps `self.trace` (config)
    /// immutable.
    pub fn print_reading<W: Write>(
        &mut self,
        reading: ReadingId,
        output: &mut W,
        sub: usize,
        trace: bool,
    ) {
        let (noprint, deleted, baseform, parent_cid) = {
            let r = self.doc.store.readings.get(reading.0);
            (
                r.noprint,
                r.deleted,
                r.baseform.unwrap_or(TagHash(0)),
                r.parent,
            )
        };
        if noprint {
            return;
        }
        if deleted {
            if !trace {
                return;
            }
            u_fputc(';', output);
        }
        for _ in 0..sub {
            u_fputc('\t', output);
        }
        let parent_cid = parent_cid.expect("reading has no parent cohort");
        let wordform_hash = {
            let wf = self.doc.store.cohorts.get(parent_cid.0).wordform;
            wf.map(|t| self.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0))
        };

        if baseform != TagHash(0) {
            let tid = tag_by_hash(self.grammar, baseform);
            let _ = write!(output, "{}", self.grammar.single_tags_list[tid.0].tag);
        }

        let tags_list: Vec<u32> = self.doc.store.readings.get(reading.0).tags_list.clone();
        let mut unique: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        let mut mappings: Vec<TagId> = Vec::new();
        for tter in tags_list {
            let tter = TagHash(tter);
            if (!self.cfg.show_end_tags && tter == self.cfg.endtag) || tter == self.cfg.begintag {
                continue;
            }
            if tter == baseform || tter == wordform_hash {
                continue;
            }
            if self.cfg.unique_tags {
                if unique.contains(&tter.get()) {
                    continue;
                }
                unique.insert(tter.get());
            }
            let tid = tag_by_hash(self.grammar, tter);
            let ttype = self.grammar.single_tags_list[tid.0].r#type;
            if ttype.intersects(T_DEPENDENCY) && self.doc.deps.has_dep && !self.cfg.dep_original {
                continue;
            }
            if ttype.intersects(T_RELATION) && self.doc.deps.has_relations {
                continue;
            }
            if ttype.intersects(T_MAPPING) {
                mappings.push(tid);
                continue;
            }
            let _ = write!(output, " {}", self.grammar.single_tags_list[tid.0].tag);
        }
        for tid in mappings {
            let _ = write!(output, " {}", self.grammar.single_tags_list[tid.0].tag);
        }

        // --- dependency annotation ---
        let parent_removed = self
            .doc
            .store
            .cohorts
            .get(parent_cid.0)
            .r#type
            .intersects(CT_REMOVED);
        if self.doc.deps.has_dep && !parent_removed {
            {
                let c = self.doc.store.cohorts.get_mut(parent_cid.0);
                if c.dep_self.is_none() {
                    c.dep_self = Some(c.global_number);
                }
            }
            let (p_global, p_local, p_dep_parent, p_sw) = {
                let c = self.doc.store.cohorts.get(parent_cid.0);
                (c.global_number, c.local_number, c.dep_parent, c.parent)
            };
            let mut pr = parent_cid;
            if let Some(pdp) = p_dep_parent {
                if pdp == GlobalNumber(0) {
                    // parent->parent->cohorts[0]
                    if let Some(sw) = p_sw {
                        pr = self.doc.store.single_windows.get(sw.0).cohorts[0];
                    }
                } else if let Some(&mapped) = self.doc.cohorts.cohort_map.get(&pdp) {
                    pr = mapped;
                }
            }
            let arrow = if self.cfg.unicode_tags {
                "\u{2192}"
            } else {
                "->"
            };
            if self.cfg.dep_absolute {
                let pr_global = self.doc.store.cohorts.get(pr.0).global_number;
                let _ = write!(output, " #{p_global}{arrow}{pr_global}");
            } else if !self.doc.dep_has_spanned {
                let pr_local = self.doc.store.cohorts.get(pr.0).local_number;
                let _ = write!(output, " #{p_local}{arrow}{pr_local}");
            } else {
                let w = self.dep_span_width();
                let p_win = p_sw
                    .map(|s| self.doc.store.single_windows.get(s.0).number)
                    .unwrap_or(0);
                if p_dep_parent.is_none() {
                    let _ = write!(
                        output,
                        " #{a}{b:0w$}{arrow}{c}{d:0w$}",
                        a = p_win,
                        b = p_local,
                        c = p_win,
                        d = p_local,
                        w = w
                    );
                } else {
                    let (pr_local, pr_win) = {
                        let c = self.doc.store.cohorts.get(pr.0);
                        let win = c
                            .parent
                            .map(|s| self.doc.store.single_windows.get(s.0).number)
                            .unwrap_or(0);
                        (c.local_number, win)
                    };
                    let _ = write!(
                        output,
                        " #{a}{b:0w$}{arrow}{c}{d:0w$}",
                        a = p_win,
                        b = p_local,
                        c = pr_win,
                        d = pr_local,
                        w = w
                    );
                }
            }
        }

        // --- ID + relations ---
        let (p_related, p_global2, relations) = {
            let c = self.doc.store.cohorts.get(parent_cid.0);
            (
                c.r#type.intersects(CT_RELATED),
                c.global_number,
                c.relations.clone(),
            )
        };
        if self.cfg.print_ids || p_related {
            let _ = write!(output, " ID:{p_global2}");
            for (rel_hash, targets) in relations.iter() {
                for siter in targets.iter() {
                    let tid = tag_by_hash(self.grammar, TagHash(*rel_hash));
                    let _ = write!(
                        output,
                        " R:{}:{siter}",
                        self.grammar.single_tags_list[tid.0].tag
                    );
                }
            }
        }

        if trace {
            let hit_by: Vec<u32> = self.doc.store.readings.get(reading.0).hit_by.clone();
            for hb in hit_by {
                u_fputc(' ', output);
                self.print_trace(output, hb);
            }
        }

        u_fputc('\n', output);

        let next = self.doc.store.readings.get(reading.0).next;
        if let Some(next_id) = next {
            self.doc.store.readings.get_mut(next_id.0).deleted = deleted;
            self.print_reading(next_id, output, sub + 1, trace);
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
    /// C++ `virtual void printCohort(Cohort* cohort, std::ostream& output,
    /// bool profiling = false)`.
    ///
    /// `trace` is the effective trace flag threaded down from the caller
    /// (normally `self.trace`; the profiling printers pass `false`), replacing
    /// the C++ `swapper<bool>` mutation of the shared `trace` member.
    pub fn print_cohort<W: Write>(
        &mut self,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
        trace: bool,
    ) {
        let local_number = self.doc.store.cohorts.get(cohort.0).local_number;
        // `goto removed` from local_number == 0 skips the entire main body.
        if local_number != 0 {
            if profiling && Some(cohort) == self.scratch.rule_target {
                let _ = writeln!(output, "# RULE TARGET BEGIN");
            }

            let wblank = self.doc.store.cohorts.get(cohort.0).wblank.clone();
            if !wblank.is_empty() {
                self.print_plain_text_line(&wblank, output);
                if !isnl(wblank.chars().next_back().unwrap_or('\0')) {
                    u_fputc('\n', output);
                }
            }

            let mut removed_goto = false;
            if self
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_REMOVED)
            {
                if !trace || self.cfg.trace_no_removed {
                    removed_goto = true;
                } else {
                    u_fputc(';', output);
                    u_fputc(' ', output);
                }
            }

            if !removed_goto {
                let (wf_tag, wf_hash) = {
                    let wf = self
                        .doc
                        .store
                        .cohorts
                        .get(cohort.0)
                        .wordform
                        .expect("cohort wordform");
                    let t = &self.grammar.single_tags_list[wf.0];
                    (t.tag.clone(), t.hash)
                };
                let _ = write!(output, "{wf_tag}");
                if let Some(wr) = self.doc.store.cohorts.get(cohort.0).wread {
                    let tags: Vec<u32> = self.doc.store.readings.get(wr.0).tags_list.clone();
                    for tter in tags {
                        let tter = TagHash(tter);
                        if tter == wf_hash {
                            continue;
                        }
                        let tid = tag_by_hash(self.grammar, tter);
                        let _ = write!(output, " {}", self.grammar.single_tags_list[tid.0].tag);
                    }
                }
                u_fputc('\n', output);

                if !profiling {
                    unignore_all(&mut self.doc.store, cohort);
                    if !self.cfg.split_mappings {
                        self.merge_mappings(cohort);
                    }
                }

                // std::sort(readings, cmp_number)
                let mut readings: Vec<ReadingId> =
                    self.doc.store.cohorts.get(cohort.0).readings.clone();
                sort_readings(&self.doc.store, &mut readings);
                self.doc.store.cohorts.get_mut(cohort.0).readings = readings.clone();
                for r in readings {
                    self.print_reading(r, output, 1, trace);
                }

                if trace && !self.cfg.trace_no_removed {
                    let mut delayed: Vec<ReadingId> =
                        self.doc.store.cohorts.get(cohort.0).delayed.clone();
                    sort_readings(&self.doc.store, &mut delayed);
                    self.doc.store.cohorts.get_mut(cohort.0).delayed = delayed.clone();
                    for r in delayed {
                        self.print_reading(r, output, 1, trace);
                    }
                    let mut del: Vec<ReadingId> =
                        self.doc.store.cohorts.get(cohort.0).deleted.clone();
                    sort_readings(&self.doc.store, &mut del);
                    self.doc.store.cohorts.get_mut(cohort.0).deleted = del.clone();
                    for r in del {
                        self.print_reading(r, output, 1, trace);
                    }
                }
            }
        }

        // removed:
        let text = self.doc.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() && text.chars().any(|c| !self.is_ws(c)) {
            self.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        if profiling && Some(cohort) == self.scratch.rule_target {
            let _ = writeln!(output, "# RULE TARGET END");
        }
    }

    /// C++ `UString::find_first_not_of(ws)` membership: is `c` in the (NUL-
    /// terminated) whitespace set `ws`?
    fn is_ws(&self, c: char) -> bool {
        for &w in &self.cfg.ws {
            if w == '\0' {
                break;
            }
            if w == c {
                return true;
            }
        }
        false
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-single-window-fn]
    /// C++ `virtual void printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling = false)`.
    ///
    /// `trace` is the effective trace flag threaded down to `print_cohort` /
    /// `print_reading` (normally `self.trace`; the profiling printers pass
    /// `false`), replacing the C++ `swapper<bool>` mutation of `trace`.
    pub fn print_single_window<W: Write>(
        &mut self,
        window: SwId,
        output: &mut W,
        profiling: bool,
        trace: bool,
    ) {
        // (The C++ virtual dispatch to the MweSplit / FormatConverter
        // overrides is the StreamFormat strategy; this is the base CG
        // implementation.)
        let (vars_output, all_cohorts, text, text_post, flush_after) = {
            let w = self.doc.store.single_windows.get(window.0);
            (
                w.variables_output.iter().copied().collect::<Vec<u32>>(),
                w.all_cohorts.clone(),
                w.text.clone(),
                w.text_post.clone(),
                w.flush_after,
            )
        };

        for var in vars_output {
            let key_tag = {
                let tid = tag_by_hash(self.grammar, TagHash(var));
                self.grammar.single_tags_list[tid.0].tag.clone()
            };
            let value_hash: Option<u32> = {
                let w = self.doc.store.single_windows.get(window.0);
                let it = w.variables_set.find(var);
                if it != w.variables_set.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            let mut cmd_buf = String::new();
            match value_hash {
                Some(vh) => {
                    if vh != self.grammar.tag_any {
                        let vtid = tag_by_hash(self.grammar, TagHash(vh));
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key_tag);
                        cmd_buf.push('=');
                        cmd_buf.push_str(&self.grammar.single_tags_list[vtid.0].tag);
                        cmd_buf.push('>');
                    } else {
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key_tag);
                        cmd_buf.push('>');
                    }
                }
                None => {
                    cmd_buf.push_str(STR_CMD_REMVAR);
                    cmd_buf.push_str(&key_tag);
                    cmd_buf.push('>');
                }
            }
            self.print_stream_command(&cmd_buf, output);
        }

        if !text.is_empty() && text.chars().any(|c| !self.is_ws(c)) {
            self.print_plain_text_line(&text, output);
        }

        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling, trace);
        }

        if !text_post.is_empty() && text_post.chars().any(|c| !self.is_ws(c)) {
            self.print_plain_text_line(&text_post, output);
        }

        if flush_after {
            self.print_stream_command(STR_CMD_FLUSH, output);
        }
        u_fflush(output);
    }

    // =======================================================================
    // pipeOut* (binary EXTERNAL serialisation, ostream/Process out)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-reading-fn]
    /// C++ `void pipeOutReading(const Reading* reading, std::ostream& output)`.
    pub fn pipe_out_reading<W: Write>(&self, reading: ReadingId, output: &mut W) {
        let mut ss: Vec<u8> = Vec::new();

        let r = self.doc.store.readings.get(reading.0);
        let mut flags: u32 = 0;
        if r.noprint {
            flags |= 1 << 1;
        }
        if r.deleted {
            flags |= 1 << 2;
        }
        if r.baseform.is_some() {
            flags |= 1 << 3;
        }
        write_raw(&mut ss, flags);

        if r.baseform.is_some() {
            let tid = tag_by_hash(self.grammar, r.baseform.unwrap_or(TagHash(0)));
            write_utf8_raw(&mut ss, &self.grammar.single_tags_list[tid.0].tag);
        }

        let wordform_hash = self
            .doc
            .store
            .cohorts
            .get(r.parent.expect("reading parent").0)
            .wordform
            .map(|t| self.grammar.single_tags_list[t.0].hash)
            .unwrap_or(TagHash(0));

        let mut cs: u32 = 0;
        for &tter in &r.tags_list {
            let tter = TagHash(tter);
            if r.baseform == Some(tter) || tter == wordform_hash {
                continue;
            }
            let tid = tag_by_hash(self.grammar, tter);
            if self.grammar.single_tags_list[tid.0]
                .r#type
                .intersects(T_DEPENDENCY)
                && self.doc.deps.has_dep
            {
                continue;
            }
            cs += 1;
        }
        write_raw(&mut ss, cs);
        for &tter in &r.tags_list {
            let tter = TagHash(tter);
            if r.baseform == Some(tter) || tter == wordform_hash {
                continue;
            }
            let tid = tag_by_hash(self.grammar, tter);
            if self.grammar.single_tags_list[tid.0]
                .r#type
                .intersects(T_DEPENDENCY)
                && self.doc.deps.has_dep
            {
                continue;
            }
            write_utf8_raw(&mut ss, &self.grammar.single_tags_list[tid.0].tag);
        }

        let cs = ui32(ss.len());
        write_raw(output, cs);
        let _ = output.write_all(&ss);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-cohort-fn]
    /// C++ `void pipeOutCohort(const Cohort* cohort, std::ostream& output)`.
    pub fn pipe_out_cohort<W: Write>(&self, cohort: CohortId, output: &mut W) {
        let mut ss: Vec<u8> = Vec::new();

        let c = self.doc.store.cohorts.get(cohort.0);
        write_raw(&mut ss, c.global_number.get());

        let mut flags: u32 = 0;
        if !c.text.is_empty() {
            flags |= 1 << 0;
        }
        if self.doc.deps.has_dep && c.dep_parent.is_some() {
            flags |= 1 << 1;
        }
        write_raw(&mut ss, flags);

        if self.doc.deps.has_dep
            && let Some(dp) = c.dep_parent
        {
            write_raw(&mut ss, dp.get());
        }

        let wf = c.wordform.expect("cohort wordform");
        write_utf8_raw(&mut ss, &self.grammar.single_tags_list[wf.0].tag);

        let cs = ui32(c.readings.len());
        write_raw(&mut ss, cs);
        let readings: Vec<ReadingId> = c.readings.clone();
        let text = c.text.clone();
        for rter1 in readings {
            self.pipe_out_reading(rter1, &mut ss);
        }
        if !text.is_empty() {
            write_utf8_raw(&mut ss, &text);
        }

        let cs = ui32(ss.len());
        write_raw(output, cs);
        let _ = output.write_all(&ss);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-single-window-fn]
    /// C++ `void pipeOutSingleWindow(const SingleWindow& window, Process& output)`.
    pub fn pipe_out_single_window(&self, window: SwId, output: &mut Process) {
        let mut ss: Vec<u8> = Vec::new();

        let (number, cohorts) = {
            let w = self.doc.store.single_windows.get(window.0);
            (w.number, w.cohorts.clone())
        };
        write_raw(&mut ss, number);

        let cs = ui32(cohorts.len()) - 1;
        write_raw(&mut ss, cs);

        for c in 1..(cs + 1) {
            self.pipe_out_cohort(cohorts[c as usize], &mut ss);
        }

        let cs = ui32(ss.len());
        {
            let mut pw = ProcWrite(output);
            write_raw(&mut pw, cs);
        }
        let _ = output.write(&ss, ss.len());
        output.flush();
    }

    // =======================================================================
    // pipeIn* (binary EXTERNAL deserialisation, Process in)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-reading-fn]
    /// C++ `void pipeInReading(Reading* reading, Process& input, bool force)`.
    /// The debug `u_fprintf(ux_stderr, ...)` traces are elided (`ux_stderr`
    /// placeholder). `reflowReading` lives in the empty reflow.rs partial.
    pub fn pipe_in_reading(&mut self, reading: ReadingId, input: &mut Process, force: bool) {
        let cs: u32 = read_raw(&mut ProcRead(input));

        let mut buf = vec![0u8; cs as usize];
        let _ = input.read(&mut buf, cs as usize);
        let mut ss = std::io::Cursor::new(buf);

        let flags: u32 = read_raw(&mut ss);

        // Not marked modified -> skip the heavy lifting.
        if !force && (flags & (1 << 0)) == 0 {
            return;
        }

        {
            let r = self.doc.store.readings.get_mut(reading.0);
            r.noprint = (flags & (1 << 1)) != 0;
            r.deleted = (flags & (1 << 2)) != 0;
        }

        if flags & (1 << 3) != 0 {
            let str = read_utf8_raw(&mut ss);
            let baseform = self
                .doc
                .store
                .readings
                .get(reading.0)
                .baseform
                .unwrap_or(TagHash(0));
            let cur = {
                let tid = tag_by_hash(self.grammar, baseform);
                self.grammar.single_tags_list[tid.0].tag.clone()
            };
            if str != cur {
                let tag = self.add_tag(&str, crate::tag::TagType::empty());
                self.doc.store.readings.get_mut(reading.0).baseform =
                    Some(self.grammar.single_tags_list[tag.0].hash);
            }
        } else {
            self.doc.store.readings.get_mut(reading.0).baseform = None;
        }

        let (wordform_hash, baseform) = {
            let r = self.doc.store.readings.get(reading.0);
            let wf = self
                .doc
                .store
                .cohorts
                .get(r.parent.expect("reading parent").0)
                .wordform
                .map(|t| self.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0));
            (wf, r.baseform.unwrap_or(TagHash(0)))
        };
        {
            let r = self.doc.store.readings.get_mut(reading.0);
            r.tags_list.clear();
            r.tags_list.push(wordform_hash.get());
            if baseform != TagHash(0) {
                r.tags_list.push(baseform.get());
            }
        }

        let cs: u32 = read_raw(&mut ss);
        for _ in 0..cs {
            let str = read_utf8_raw(&mut ss);
            let tag = self.add_tag(&str, crate::tag::TagType::empty());
            let hash = self.grammar.single_tags_list[tag.0].hash;
            self.doc
                .store
                .readings
                .get_mut(reading.0)
                .tags_list
                .push(hash.get());
        }

        // reflowReading(*reading) — direct now that the pipe fns use
        // self.doc.store (the old take/swap dance is gone).
        self.reflow_reading(reading);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
    /// C++ `void pipeInCohort(Cohort* cohort, Process& input)`.
    pub fn pipe_in_cohort(&mut self, cohort: CohortId, input: &mut Process) {
        let _packet_len: u32 = read_raw(&mut ProcRead(input));

        let cs: u32 = read_raw(&mut ProcRead(input));
        let global_number = self.doc.store.cohorts.get(cohort.0).global_number.get();
        if cs != global_number {
            // "Error: External returned data for cohort ... but we expected ...!"
            cg3_quit(1, Some(file!()), self.doc.num_lines);
        }

        let flags: u32 = read_raw(&mut ProcRead(input));

        if flags & (1 << 1) != 0 {
            let dp: u32 = read_raw(&mut ProcRead(input));
            self.doc.store.cohorts.get_mut(cohort.0).dep_parent = if dp == DEP_NO_PARENT {
                None
            } else {
                Some(GlobalNumber(dp))
            };
        }

        let mut force_readings = false;
        let str = read_utf8_raw(&mut ProcRead(input));
        let cur_wf = self
            .doc
            .store
            .cohorts
            .get(cohort.0)
            .wordform
            .map(|t| self.grammar.single_tags_list[t.0].tag.clone())
            .unwrap_or_default();
        if str != cur_wf {
            let tag = self.add_tag(&str, crate::tag::TagType::empty());
            self.doc.store.cohorts.get_mut(cohort.0).wordform = Some(tag);
            force_readings = true;
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        for i in 0..cs {
            let rid = self.doc.store.cohorts.get(cohort.0).readings[i as usize];
            self.pipe_in_reading(rid, input, force_readings);
        }

        if flags & (1 << 0) != 0 {
            let text = read_utf8_raw(&mut ProcRead(input));
            self.doc.store.cohorts.get_mut(cohort.0).text = text;
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
    /// C++ `void pipeInSingleWindow(SingleWindow& window, Process& input)`.
    pub fn pipe_in_single_window(&mut self, window: SwId, input: &mut Process) {
        let cs: u32 = read_raw(&mut ProcRead(input));
        if cs == 0 {
            return;
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        let number = self.doc.store.single_windows.get(window.0).number;
        if cs != number {
            // "Error: External returned data for window ... but we expected ...!"
            cg3_quit(1, Some(file!()), self.doc.num_lines);
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        for i in 0..cs {
            let cid = self.doc.store.single_windows.get(window.0).cohorts[(i + 1) as usize];
            self.pipe_in_cohort(cid, input);
        }
    }

    // =======================================================================
    // error (4 C++ overloads -> 3 Rust fns; two UChar*/char* single-arg
    // overloads collapse to &str)
    // =======================================================================

}

impl super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.error-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.error-fn]
    /// C++ `void error(const char* str, const UChar* p)` — `p` ignored. The C
    /// printf format `str` is filled with `(label, line, label)`; since the sink
    /// (`ux_stderr`) is a placeholder and `str` is a runtime printf template
    /// (not portable to Rust's compile-time `format!`), the label/line are
    /// selected faithfully and emission is deferred. Returns the chosen
    /// `(label, line)` for callers/tests.
    pub fn error(&self, _str: &str, _p: Option<&str>) -> (&'static str, u32) {
        self.error_labels()
    }

    /// C++ `error(str, s, p)` — the `const char* s` and `const UChar* s`
    /// overloads collapse to one `&str s` (spliced between the first label and
    /// the line in the format). Same deferred-emission note as [`error`].
    pub fn error_s(&self, _str: &str, _s: &str, _p: Option<&str>) -> (&'static str, u32) {
        self.error_labels()
    }

    /// C++ `error(str, s, S, p)` — two spliced strings. Same deferred-emission
    /// note as [`error`].
    pub fn error_ss(
        &self,
        _str: &str,
        _s: &str,
        _big_s: &str,
        _p: Option<&str>,
    ) -> (&'static str, u32) {
        self.error_labels()
    }

    /// Shared label/line selection for the `error(...)` family: `("RT RULE",
    /// current_rule->line)` when a current rule with a non-zero line is set,
    /// else `("RT INPUT", numLines)`.
    fn error_labels(&self) -> (&'static str, u32) {
        if let Some(rid) = self.scratch.current_rule {
            let line = self.grammar.rule_by_number[rid.0].line;
            if line != 0 {
                return ("RT RULE", line);
            }
        }
        ("RT INPUT", self.doc.num_lines)
    }

    // =======================================================================
    // setOptions
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-options-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-options-fn]
    /// C++ `void setOptions(UConverter* conv)` — copies the parsed CLI option
    /// table into the applicator flags.
    ///
    /// DIVERGENCE: the C++ reads a global `Options::options[]`; here the parsed
    /// table is passed in (`options: &options_t`). The `UConverter*` is dropped
    /// — option values are already UTF-8 `String`s, so `ucnv_toUChars` is the
    /// identity.
    pub fn set_options(&mut self, options: &options_t) -> Result<(), crate::error::Cg3Error> {
        let occ = |o: OPTIONS| options[o as usize].does_occur;
        let val = |o: OPTIONS| options[o as usize].value.as_str();

        if occ(OPTIONS::ALWAYS_SPAN) {
            self.cfg.always_span = true;
        }
        self.cfg.unicode_tags = false;
        if occ(OPTIONS::UNICODE_TAGS) {
            self.cfg.unicode_tags = true;
        }
        self.cfg.unique_tags = false;
        if occ(OPTIONS::UNIQUE_TAGS) {
            self.cfg.unique_tags = true;
        }
        self.cfg.apply_mappings = true;
        if occ(OPTIONS::NOMAPPINGS) {
            self.cfg.apply_mappings = false;
        }
        self.cfg.apply_corrections = true;
        if occ(OPTIONS::NOCORRECTIONS) {
            self.cfg.apply_corrections = false;
        }
        self.cfg.no_before_sections = false;
        if occ(OPTIONS::NOBEFORESECTIONS) {
            self.cfg.no_before_sections = true;
        }
        self.cfg.no_sections = false;
        if occ(OPTIONS::NOSECTIONS) {
            self.cfg.no_sections = true;
        }
        self.cfg.no_after_sections = false;
        if occ(OPTIONS::NOAFTERSECTIONS) {
            self.cfg.no_after_sections = true;
        }
        self.cfg.r#unsafe = false;
        if occ(OPTIONS::UNSAFE) {
            self.cfg.r#unsafe = true;
        }
        if occ(OPTIONS::ORDERED) {
            self.cfg.ordered = true;
        }
        if occ(OPTIONS::TRACE) {
            self.cfg.trace = true;
            if !val(OPTIONS::TRACE).is_empty() {
                self.set_opts_ranged_interval(val(OPTIONS::TRACE), true, false);
            }
        }
        if occ(OPTIONS::TRACE_NAME_ONLY) {
            self.cfg.trace = true;
            self.cfg.trace_name_only = true;
        }
        if occ(OPTIONS::TRACE_NO_REMOVED) {
            self.cfg.trace = true;
            self.cfg.trace_no_removed = true;
        }
        if occ(OPTIONS::TRACE_ENCL) {
            self.cfg.trace = true;
            self.cfg.trace_encl = true;
        }
        if occ(OPTIONS::PIPE_DELETED) {
            self.cfg.pipe_deleted = true;
        }
        if occ(OPTIONS::DRYRUN) {
            self.cfg.dry_run = true;
        }
        if occ(OPTIONS::SINGLERUN) {
            self.cfg.section_max_count = 1;
        }
        if occ(OPTIONS::MAXRUNS) {
            self.cfg.section_max_count = stoul(val(OPTIONS::MAXRUNS));
        }
        if occ(OPTIONS::SECTIONS) {
            g_app_set_opts_ranged(val(OPTIONS::SECTIONS), &mut self.cfg.sections, true);
        }
        if occ(OPTIONS::RULES) {
            self.set_opts_ranged_interval_valid(val(OPTIONS::RULES), true);
        }
        if occ(OPTIONS::RULE) {
            let v = val(OPTIONS::RULE);
            let first = v.chars().next().unwrap_or('\0');
            if first.is_ascii_digit() {
                self.cfg.valid_rules.insert_sorted(stoi(v) as u32);
            } else {
                // ucnv_toUChars is identity for UTF-8; compare rule names.
                for i in 0..self.grammar.rule_by_number.capacity() {
                    if let Some(rule) = self.grammar.rule_by_number.try_get(i)
                        && rule.name == v
                    {
                        self.cfg.valid_rules.insert_sorted(rule.number);
                    }
                }
            }
        }
        if occ(OPTIONS::DEBUG_RULES) {
            self.set_opts_ranged_interval_debug(val(OPTIONS::DEBUG_RULES), false);
        }
        if occ(OPTIONS::VERBOSE) {
            self.cfg.verbosity_level = if !val(OPTIONS::VERBOSE).is_empty() {
                stoul(val(OPTIONS::VERBOSE))
            } else {
                1
            };
        }
        if occ(OPTIONS::DODEBUG) {
            self.cfg.debug_level = if !val(OPTIONS::DODEBUG).is_empty() {
                stoul(val(OPTIONS::DODEBUG))
            } else {
                1
            };
            // C++ `std::cerr << "Debug level set to " << debug_level`: deferred.
        }
        if occ(OPTIONS::PRINT_IDS) {
            self.cfg.print_ids = true;
        }
        if occ(OPTIONS::PRINT_DEP) {
            self.doc.deps.has_dep = true;
        }
        if occ(OPTIONS::NUM_WINDOWS) {
            self.cfg.num_windows = stoul(val(OPTIONS::NUM_WINDOWS));
        }
        if occ(OPTIONS::SOFT_LIMIT) {
            self.cfg.soft_limit = stoul(val(OPTIONS::SOFT_LIMIT));
        }
        if occ(OPTIONS::HARD_LIMIT) {
            self.cfg.hard_limit = stoul(val(OPTIONS::HARD_LIMIT));
        }
        if occ(OPTIONS::TEXT_DELIMIT) {
            let rx = if !val(OPTIONS::TEXT_DELIMIT).is_empty() {
                val(OPTIONS::TEXT_DELIMIT).to_string()
            } else {
                STR_TEXTDELIM_DEFAULT.to_string()
            };
            self.set_text_delimiter(rx)?;
        }
        if occ(OPTIONS::DEP_DELIMIT) {
            self.cfg.dep_delimit = if !val(OPTIONS::DEP_DELIMIT).is_empty() {
                stoul(val(OPTIONS::DEP_DELIMIT))
            } else {
                10
            };
            self.cfg.parse_dep = true;
        }
        if occ(OPTIONS::DEP_ABSOLUTE) {
            self.cfg.dep_absolute = true;
        }
        if occ(OPTIONS::DEP_ORIGINAL) {
            self.cfg.dep_original = true;
        }
        if occ(OPTIONS::DEP_ALLOW_LOOPS) {
            self.cfg.dep_block_loops = false;
        }
        if occ(OPTIONS::DEP_BLOCK_CROSSING) {
            self.cfg.dep_block_crossing = true;
        }
        if occ(OPTIONS::MAGIC_READINGS) {
            self.cfg.allow_magic_readings = false;
        }
        if occ(OPTIONS::NO_PASS_ORIGIN) {
            self.cfg.no_pass_origin = true;
        }
        if occ(OPTIONS::SPLIT_MAPPINGS) {
            self.cfg.split_mappings = true;
        }
        if occ(OPTIONS::SHOW_END_TAGS) {
            self.cfg.show_end_tags = true;
        }
        if occ(OPTIONS::NO_BREAK) {
            self.cfg.add_spacing = false;
        }
        Ok(())
    }

    /// `GAppSetOpts_ranged(value, trace_rules, fill)` bridge: the ported helper
    /// fills a `Vec<u32>`, so expand there and insert into the interval vector.
    fn set_opts_ranged_interval(&mut self, value: &str, _default_true: bool, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.cfg.trace_rules.insert(v);
        }
    }
    fn set_opts_ranged_interval_valid(&mut self, value: &str, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.cfg.valid_rules.insert(v);
        }
    }
    fn set_opts_ranged_interval_debug(&mut self, value: &str, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.cfg.debug_rules.insert(v);
        }
    }

    // =======================================================================
    // printDebugRule / addProfilingExample / profileRuleContext (inline .hpp)
    // =======================================================================

}

impl Engine<'_> {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]
    /// C++ inline `void printDebugRule(const Rule& rule, bool target, bool cntx)`.
    /// Renders the whole in-flight window set (profiling mode) with `trace`
    /// force-disabled, into a buffer written to stderr (the C++ `ux_stderr`).
    /// The C++ `swapper<bool>(true, trace, ttrace=false)` save/restore of the
    /// shared `trace` member is replaced by passing `trace = false` down the
    /// print chain, so `self.trace` (config) is never mutated.
    pub fn print_debug_rule(&mut self, rule: RuleId, target: bool, cntx: bool) {
        let mut buf: Vec<u8> = Vec::new();
        let line = self.grammar.rule_by_number[rule.0].line;
        let _ = writeln!(
            &mut buf,
            "# ===== BEGIN RULE {}{}{} =====",
            line,
            if target {
                " TARGET-MATCH"
            } else {
                " TARGET-FAIL"
            },
            if cntx {
                " CONTEXT-MATCH"
            } else {
                " CONTEXT-FAIL"
            }
        );

        let _ = writeln!(&mut buf, "# PREVIOUS WINDOWS");
        for i in 0..self.doc.stream.previous.len() {
            let s = self.doc.stream.previous[i];
            self.print_single_window(s, &mut buf, true, false);
        }
        let _ = writeln!(&mut buf, "# CURRENT WINDOW");
        if let Some(cur) = self.doc.stream.current {
            self.print_single_window(cur, &mut buf, true, false);
        }
        let _ = writeln!(&mut buf, "# NEXT WINDOWS");
        for i in 0..self.doc.stream.next.len() {
            let s = self.doc.stream.next[i];
            self.print_single_window(s, &mut buf, true, false);
        }

        let _ = writeln!(&mut buf, "# ===== END RULE {line} =====");

        // u_fprintf(ux_stderr, "%s", buf) — a raw stream dump (window data),
        // not a log event: write it straight to stderr like the C++.
        let _ = std::io::stderr().write_all(&buf);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]
    /// C++ template `void addProfilingExample(T& item)`. Renders the whole
    /// in-flight window set (previous / current / next, trace force-disabled)
    /// into a buffer, interns it in the profiler string table, and stores the id
    /// into `entries[key].example_window`. (The C++ passes the entry by
    /// reference; the port passes its `key` — same entry, borrow-checker-
    /// friendly.) Caller guarantees `self.diag.profiler` is `Some` and the entry
    /// exists. The C++ `swapper<bool>(true, trace, ttrace=false)` save/restore
    /// of the shared `trace` member is replaced by passing `trace = false` down
    /// the print chain, so `self.trace` (config) is never mutated.
    pub(super) fn add_profiling_example(&mut self, key: crate::profiler::Key) {
        let mut buf: Vec<u8> = Vec::new();
        let _ = writeln!(&mut buf, "# PREVIOUS WINDOWS");
        for i in 0..self.doc.stream.previous.len() {
            let s = self.doc.stream.previous[i];
            self.print_single_window(s, &mut buf, true, false);
        }
        let _ = writeln!(&mut buf, "# CURRENT WINDOW");
        if let Some(cur) = self.doc.stream.current {
            self.print_single_window(cur, &mut buf, true, false);
        }
        let _ = writeln!(&mut buf, "# NEXT WINDOWS");
        for i in 0..self.doc.stream.next.len() {
            let s = self.doc.stream.next[i];
            self.print_single_window(s, &mut buf, true, false);
        }

        let p = self.diag.profiler.as_mut().unwrap();
        let sz = p.add_string(&String::from_utf8_lossy(&buf));
        if let Some(e) = p.entries.get_mut(&key) {
            e.example_window = sz;
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.profile-rule-context-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.profile-rule-context-fn]
    /// C++ inline `void profileRuleContext(bool test_good, const Rule* rule,
    /// const ContextualTest* test)`. Guarded on `profiler`: looks up the
    /// `{ET_CONTEXT, test->hash}` entry (parser-registered; a miss is a no-op),
    /// and — when the test outcome agrees with its `POS_NEGATE` polarity — bumps
    /// `num_match`, seeds `example_window` via [`add_profiling_example`], and
    /// increments `rule_contexts[(rule->number + 1, test->hash)]`; otherwise
    /// bumps `num_fail`.
    ///
    /// [`add_profiling_example`]: GrammarApplicator::add_profiling_example
    pub fn profile_rule_context(&mut self, test_good: bool, rule: RuleId, test: CtxId) {
        if self.diag.profiler.is_none() {
            return;
        }
        let test_hash = self.grammar.contexts_arena[test.0].hash;
        let test_pos = self.grammar.contexts_arena[test.0].pos;
        let rule_number = self.grammar.rule_by_number.get(rule.0).number;
        let key = crate::profiler::Key {
            r#type: crate::profiler::ET_CONTEXT,
            id: test_hash,
        };
        let p = self.diag.profiler.as_mut().unwrap();
        let Some(t) = p.entries.get_mut(&key) else {
            return;
        };
        if (test_good && (!test_pos.intersects(POS_NEGATE)))
            || (!test_good && (test_pos.intersects(POS_NEGATE)))
        {
            t.num_match += 1;
            let need_example = t.example_window == 0;
            *p.rule_contexts
                .entry((rule_number + 1, test_hash))
                .or_insert(0) += 1;
            if need_example {
                self.add_profiling_example(key);
            }
        } else {
            t.num_fail += 1;
        }
    }
}

/// C++ `std::sort(list, Reading::cmp_number)` — sorts reading ids ascending by
/// `number`, tie-broken by `hash`. `cmp_number` is a strict-less predicate;
/// bridged to `Ordering` via the two-way comparison.
fn sort_readings(store: &RuntimeStore, list: &mut [ReadingId]) {
    list.sort_by(|&a, &b| {
        let ra = store.readings.get(a.0);
        let rb = store.readings.get(b.0);
        if Reading::cmp_number(ra, rb) {
            std::cmp::Ordering::Less
        } else if Reading::cmp_number(rb, ra) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}

impl Engine<'_> {
    // =======================================================================
    // addTag (Tag* internal overload + UChar*/type public overload)
    // =======================================================================

    /// C++ (unspecced internal) `Tag* GrammarApplicator::addTag(Tag* tag)` —
    /// interns a freshly-built `Tag` value into `grammar.single_tags_list`
    /// (arena) + `grammar.single_tags` (hash → id) with the 0..9999 seed-probe
    /// dedup. Identical algorithm to `Grammar::add_tag`; kept as the applicator's
    /// own per scope. The `verbosity_level>0` seed warning is deferred I/O
    /// (`ux_stderr` placeholder). Returns the canonical `TagId`.
    fn add_tag_ptr(&mut self, mut tag: Tag) -> TagId {
        let hash = tag.rehash();
        let mut existing: Option<TagId> = None;
        let mut chosen_seed: Option<u32> = None;
        let mut seed = 0u32;
        while seed < 10000 {
            let ih = hash.wrapping_add(seed);
            let found: Option<TagId> = {
                let it = self.grammar.single_tags.find(ih.get());
                if it != self.grammar.single_tags.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            match found {
                Some(t_id) => {
                    // `t == tag` (pointer identity) is impossible for a fresh
                    // by-value tag; only the text-equality dedup applies.
                    if self.grammar.single_tags_list[t_id.0].tag == tag.tag {
                        existing = Some(t_id);
                        break;
                    }
                }
                None => {
                    chosen_seed = Some(seed);
                    break;
                }
            }
            seed += 1;
        }

        if let Some(t_id) = existing {
            // C++ `delete tag`: the incoming value drops at end of scope.
            return t_id;
        }

        let seed = chosen_seed.expect("addTag: hash seed space exhausted");
        tag.seed = seed;
        let new_hash = tag.rehash();
        let idx = self.grammar.single_tags_list.alloc(tag);
        self.grammar.single_tags_list[idx].number = idx;
        self.grammar
            .single_tags
            .insert((new_hash.get(), TagId(idx)));
        TagId(idx)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
    /// C++ `Tag* addTag(const UChar* txt, uint32_t type)` — interns a tag from
    /// text and returns its canonical `TagId`. Collapses the three C++ overloads
    /// (`const UChar*` / `const UString&` / `UStringView`), which all map onto
    /// `&str`. Returns `TagId`.
    ///
    /// The `T_VARSTRING` branch is the applicator instantiation of the
    /// `parser_helpers.hpp` template: `::CG3::parseTag(txt, 0, *this,
    /// !(type & T_PRESERVE_ESC))` — full tag parsing (prefixes, r/i/v/l/p
    /// suffixes, regex compile, numeric `<…>`), so runtime-generated tags get
    /// their T_REGEXP / T_SET / T_NUMERICAL / … semantics.
    pub fn add_tag(&mut self, txt: &str, r#type: crate::tag::TagType) -> TagId {
        // Fast path: an existing un-seeded slot whose text matches exactly.
        let thash = hash_value_ustring(txt, 0);
        {
            let it = self.grammar.single_tags.find(thash);
            if it != self.grammar.single_tags.end() {
                let tid = it.get().1;
                let t = &self.grammar.single_tags_list[tid.0];
                if !t.tag.is_empty() && t.tag == txt {
                    return tid;
                }
            }
        }

        let tag: TagId = if r#type.intersects(T_VARSTRING) {
            // C++: tag = ::CG3::parseTag(txt, 0, *this, !type.intersects(T_PRESERVE_ESC));
            // (`p = 0` — no near-context at runtime.)
            crate::parser_helpers::parse_tag(txt, &[], self, !r#type.intersects(T_PRESERVE_ESC))
        } else {
            let mut t = Tag::default();
            crate::tag::parse_tag_raw(&mut t, txt, self.grammar);
            self.add_tag_ptr(t)
        };

        let mut reflow = false;
        let (ttype, is_txt) = {
            let t = &self.grammar.single_tags_list[tag.0];
            (t.r#type, is_textual(&t.tag))
        };

        if (ttype.intersects(T_REGEXP)) && !is_txt {
            // grammar->regex_tags.insert(tag->regexp).second — the set is keyed
            // by TagId here; treat a newly-inserted id as ".second == true".
            let inserted = self.grammar.regex_tags.insert(tag);
            if inserted {
                // Scan every non-textual single_tag against every regex tag;
                // mark T_TEXTUAL on any (unanchored) match. Collect ids first to
                // avoid aliasing the arena during mutation.
                let all_tags: Vec<TagId> = (0..self.grammar.single_tags_list.capacity())
                    .filter_map(|i| self.grammar.single_tags_list.try_get(i).map(|_| TagId(i)))
                    .collect();
                let regex_ids: Vec<TagId> = self.grammar.regex_tags.iter().copied().collect();
                for titer in all_tags {
                    if self.grammar.single_tags_list[titer.0]
                        .r#type
                        .intersects(T_TEXTUAL)
                    {
                        continue;
                    }
                    let text = self.grammar.single_tags_list[titer.0].tag.clone();
                    for &rid in &regex_ids {
                        let matched = self.grammar.single_tags_list[rid.0]
                            .regexp
                            .as_ref()
                            .map(|re| re.is_match(&text))
                            .unwrap_or(false);
                        if matched {
                            self.grammar.single_tags_list[titer.0].r#type |= T_TEXTUAL;
                            reflow = true;
                        }
                    }
                }
            }
        }
        if (ttype.intersects(T_CASE_INSENSITIVE)) && !is_txt {
            // grammar->icase_tags.insert(tag).second
            let inserted = self.grammar.icase_tags.insert(tag).1;
            if inserted {
                let all_tags: Vec<TagId> = (0..self.grammar.single_tags_list.capacity())
                    .filter_map(|i| self.grammar.single_tags_list.try_get(i).map(|_| TagId(i)))
                    .collect();
                let icase_ids: Vec<TagId> = self.grammar.icase_tags.iter().copied().collect();
                for titer in all_tags {
                    if self.grammar.single_tags_list[titer.0]
                        .r#type
                        .intersects(T_TEXTUAL)
                    {
                        continue;
                    }
                    let text = self.grammar.single_tags_list[titer.0].tag.clone();
                    for &iid in &icase_ids {
                        let itext = &self.grammar.single_tags_list[iid.0].tag;
                        if ux_strCaseCompare(&text, itext) {
                            self.grammar.single_tags_list[titer.0].r#type |= T_TEXTUAL;
                            reflow = true;
                        }
                    }
                }
            }
        }
        if reflow {
            self.reflow_textuals();
        }
        tag
    }
}

/// The applicator instantiation of the C++ `parser_helpers.hpp`
/// `template<typename State> parseTag(...)` — used by
/// [`GrammarApplicator::add_tag`]'s `T_VARSTRING` branch so runtime-generated
/// tags go through the full parser (regex compile, prefixes, suffixes,
/// numerics) instead of the raw path. Implemented on the split-borrow
/// [`Engine`](super::Engine) view: the varstring branch is reached from the
/// peeled contextual matcher knot, so `parse_tag(..., self, ...)` threads an
/// `Engine`.
impl crate::parser_helpers::ParseTagState for Engine<'_> {
    fn grammar(&self) -> &Grammar {
        &*self.grammar
    }

    /// C++ `GrammarApplicator::filebase` is `nullptr` (never set) — the
    /// warnings that print it only fire on malformed tags.
    fn filebase(&self) -> &str {
        ""
    }

    /// C++ `GrammarApplicator::error(str, p)` prints `("RT RULE",
    /// current_rule->line)` / `("RT INPUT", numLines)` into the format and
    /// RETURNS (non-fatal, unlike `TextualParser::error`). The port's
    /// `error()` defers the sink, so emit a plain stderr line here. The label/
    /// line selection is `GrammarApplicator::error_labels` inlined (that helper
    /// stays `&self` on the applicator; the values it reads live on `Engine`).
    fn error_near(&mut self, _near: &[char]) {
        let (label, line) = if let Some(rid) = self.scratch.current_rule
            && self.grammar.rule_by_number[rid.0].line != 0
        {
            ("RT RULE", self.grammar.rule_by_number[rid.0].line)
        } else {
            ("RT INPUT", self.doc.num_lines)
        };
        tracing::error!("Error: parseTag failed at {label} {line}");
    }

    /// C++ `state.addTag(tag)` → `GrammarApplicator::addTag(Tag*)` — the
    /// seed-probing interner, NOT `Grammar::addTag`.
    fn add_tag(&mut self, tag: Tag) -> TagId {
        self.add_tag_ptr(tag)
    }
}
