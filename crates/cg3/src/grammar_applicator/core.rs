//! `src/GrammarApplicator.cpp` (+ the inline getters/setters/printers of
//! `src/GrammarApplicator.hpp`) — the engine construction / indexing / tag
//! interning / stream printing / EXTERNAL-pipe I/O method bodies.
//!
//! ARENA + STORE THREADING (important port note).
//! The task's ownership model says the applicator OWNS `self.store:
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
//! PLACEHOLDERS. `self.profiler`/`self.ux_stderr`/`self.ux_stdin`/`self.ux_stdout`
//! are `Option<()>` stand-ins (no Profiler module, no wired streams yet), so the
//! `error(...)`/`printDebugRule`/`addProfilingExample`/`profileRuleContext`
//! emissions are built faithfully but not flushed to a real stream (noted inline).

use std::io::{Read, Write};

use regex::RegexBuilder;

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, SwId, TagId};
use crate::contextual_test::POS_NEGATE;
use crate::cohort::{CT_RELATED, CT_REMOVED, DEP_NO_PARENT, unignore_all};
use crate::grammar::Grammar;
use crate::inlines::{
    cg3_quit, g_app_set_opts_ranged, hash_value_ustring, is_textual, isnl, read_raw, read_utf8_raw,
    ui32, ui8, write_raw, write_utf8_raw,
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
use crate::uextras::{u_fflush, u_fputc, ux_strCaseCompare};

use super::tmpl_context_t;

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
pub(super) fn tag_by_hash(grammar: &Grammar, hash: u32) -> TagId {
    let it = grammar.single_tags.find(hash);
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
    if let Some(&c) = it.peek() {
        if c == '+' || c == '-' {
            neg = c == '-';
            it.next();
        }
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
        for sv in &mut self.index_readingSet_yes {
            sv.clear(0);
        }
        for sv in &mut self.index_readingSet_no {
            sv.clear(0);
        }
        self.index_regexp_yes.clear(0);
        self.index_regexp_no.clear(0);
        self.index_icase_yes.clear(0);
        self.index_icase_no.clear(0);
    }

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
                let it = self.grammar.single_tags.find(ih);
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
        self.grammar.single_tags.insert((new_hash, TagId(idx)));
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

        let tag: TagId;
        if r#type.intersects(T_VARSTRING) {
            // C++: tag = ::CG3::parseTag(txt, 0, *this, !type.intersects(T_PRESERVE_ESC));
            // (`p = 0` — no near-context at runtime.)
            tag = crate::parser_helpers::parse_tag(
                txt,
                &[],
                self,
                !r#type.intersects(T_PRESERVE_ESC),
            );
        } else {
            let mut t = Tag::default();
            crate::tag::parse_tag_raw(&mut t, txt, &mut self.grammar);
            tag = self.add_tag_ptr(t);
        }

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
                let all_tags: Vec<TagId> =
                    (0..self.grammar.single_tags_list.capacity())
                        .filter_map(|i| self.grammar.single_tags_list.try_get(i).map(|_| TagId(i)))
                        .collect();
                let regex_ids: Vec<TagId> = self.grammar.regex_tags.iter().copied().collect();
                for titer in all_tags {
                    if self.grammar.single_tags_list[titer.0].r#type.intersects(T_TEXTUAL) {
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
                let all_tags: Vec<TagId> =
                    (0..self.grammar.single_tags_list.capacity())
                        .filter_map(|i| self.grammar.single_tags_list.try_get(i).map(|_| TagId(i)))
                        .collect();
                let icase_ids: Vec<TagId> = self.grammar.icase_tags.iter().copied().collect();
                for titer in all_tags {
                    if self.grammar.single_tags_list[titer.0].r#type.intersects(T_TEXTUAL) {
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
    pub fn set_grammar(&mut self) {
        let tb = self.add_tag(STR_BEGINTAG, crate::tag::TagType::empty());
        let te = self.add_tag(STR_ENDTAG, crate::tag::TagType::empty());
        let ts = self.add_tag(STR_DUMMY, crate::tag::TagType::empty());
        self.tag_begin = Some(tb);
        self.tag_end = Some(te);
        self.tag_subst = Some(ts);
        self.begintag = self.grammar.single_tags_list[tb.0].hash;
        self.endtag = self.grammar.single_tags_list[te.0].hash;
        self.substtag = self.grammar.single_tags_list[ts.0].hash;

        let mp: String = self.grammar.mapping_prefix.to_string();
        let k = self.add_tag("_MPREFIX", crate::tag::TagType::empty());
        self.mprefix_key = self.grammar.single_tags_list[k.0].hash;
        let v = self.add_tag(&mp, crate::tag::TagType::empty());
        self.mprefix_value = self.grammar.single_tags_list[v.0].hash;

        let n = self.grammar.sets_list.capacity() as usize;
        self.index_readingSet_yes.clear();
        self.index_readingSet_yes.resize_with(n, Default::default);
        self.index_readingSet_no.clear();
        self.index_readingSet_no.resize_with(n, Default::default);

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
                    Ok(re) => self.text_delimiters.push(re),
                    Err(_) => {
                        // "Error: uregex_open returned ... - cannot continue!"
                        cg3_quit(1, Some(file!()), self.numLines);
                    }
                }
            }
        }
    }

    // =======================================================================
    // setTextDelimiter
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
    /// C++ `void setTextDelimiter(UString rx)` — replaces the compiled
    /// text-delimiter regex from a (possibly `/.../ri`-wrapped) pattern.
    pub fn set_text_delimiter(&mut self, rx: crate::types::UString) {
        // uregex_close(r) for each: regex::Regex drops here.
        self.text_delimiters.clear();

        if rx.is_empty() {
            return;
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
            Ok(re) => self.text_delimiters.push(re),
            Err(_) => {
                cg3_quit(1, Some(file!()), self.numLines);
            }
        }
    }

    // =======================================================================
    // index
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.index-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.index-fn]
    /// C++ `void index()` — builds the per-section rule schedule and the
    /// dependency span-print patterns; runs at most once.
    pub fn index(&mut self) {
        if !self.add_spacing {
            self.ws[2] = '\n';
        }
        if self.did_index {
            return;
        }

        if self.grammar.ordered {
            self.ordered = true;
        }
        if self.grammar.has_dep || self.dep_delimit != 0 {
            self.parse_dep = true;
        }

        if !self.grammar.before_sections.is_empty() {
            let rules: Vec<RuleId> = self.grammar.before_sections.clone();
            let m = self.runsections.entry(-1).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }
        if !self.grammar.after_sections.is_empty() {
            let rules: Vec<RuleId> = self.grammar.after_sections.clone();
            let m = self.runsections.entry(-2).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }
        if !self.grammar.null_section.is_empty() {
            let rules: Vec<RuleId> = self.grammar.null_section.clone();
            let m = self.runsections.entry(-3).or_default();
            for r in rules {
                m.insert(self.grammar.rule_by_number[r.0].number);
            }
        }

        if self.sections.is_empty() {
            let smax = self.grammar.sections.len() as i32;
            let rules: Vec<RuleId> = self.grammar.rules.clone();
            for i in 0..smax {
                for &r in &rules {
                    let rule = &self.grammar.rule_by_number[r.0];
                    if rule.section < 0 || rule.section > i {
                        continue;
                    }
                    let num = rule.number;
                    self.runsections.entry(i).or_default().insert(num);
                }
            }
        } else {
            self.numsections = ui32(self.sections.len());
            let rules: Vec<RuleId> = self.grammar.rules.clone();
            let sections = self.sections.clone();
            for n in 0..self.numsections {
                for e in 0..=n {
                    for &r in &rules {
                        let rule = &self.grammar.rule_by_number[r.0];
                        if rule.section != (sections[e as usize] as i32) - 1 {
                            continue;
                        }
                        let num = rule.number;
                        self.runsections.entry(n as i32).or_default().insert(num);
                    }
                }
            }
        }

        if !self.valid_rules.empty() {
            let mut vr = crate::interval_vector::uint32IntervalVector::new();
            for i in 0..self.grammar.rule_by_number.capacity() {
                if let Some(rule) = self.grammar.rule_by_number.try_get(i) {
                    if self.valid_rules.contains(rule.line) {
                        vr.insert_sorted(rule.number);
                    }
                }
            }
            self.valid_rules = vr;
        }

        // Dependency span print patterns (state kept for parity; print_reading
        // reproduces the format directly via dep_span_width()).
        let w = ui8((self.hard_limit as f64).log10().floor() + 1.0);
        let wc = char::from_digit(w as u32, 10).unwrap_or('1');
        self.span_pattern_utf = format!(" #%u%0{wc}u\u{2192}%u%0{wc}u");
        self.span_pattern_latin = format!(" #%u%0{wc}u->%u%0{wc}u");

        self.did_index = true;
    }

    /// Dependency span zero-pad width — `floor(log10(hard_limit)) + 1`, matching
    /// the digit baked into `span_pattern_*` by [`index`].
    fn dep_span_width(&self) -> usize {
        (ui8((self.hard_limit as f64).log10().floor() + 1.0) as usize).max(1)
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
        let _ = write!(output, "{cmd}\n");
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

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
    /// C++ `void printTrace(std::ostream& output, uint32_t hit_by)`.
    pub fn print_trace<W: Write>(&self, output: &mut W, hit_by: u32) {
        if (hit_by as usize) < self.grammar.rule_by_number.capacity() as usize
            && self.grammar.rule_by_number.try_get(hit_by).is_some()
        {
            let r = &self.grammar.rule_by_number[hit_by];
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
                if let Some(txt) = self.first_maplist_tag(r.maplist) {
                    let _ = write!(output, "({txt}");
                }
                if matches!(r.r#type, K_ADDRELATIONS | K_SETRELATIONS | K_REMRELATIONS) {
                    if let Some(txt) = self.first_maplist_tag(r.sublist) {
                        let _ = write!(output, ",{txt}");
                    }
                }
                let _ = write!(output, ")");
            }
            if !self.trace_name_only || r.name.is_empty() {
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

    /// C++ `r->maplist->getNonEmpty().begin()->first->tag` — the first tag text
    /// of a rule's map/sub set (its `trie`, or `trie_special` when the trie is
    /// empty). `getNonEmpty` is not yet ported in `set.rs`; reproduced inline.
    fn first_maplist_tag(&self, set: Option<crate::arena::SetId>) -> Option<&str> {
        let sid = set?;
        let s = &self.grammar.sets_list[sid.0];
        let trie = if !s.trie.is_empty() { &s.trie } else { &s.trie_special };
        let (tid, _node) = trie.iter().next()?;
        Some(&self.grammar.single_tags_list[tid.0].tag)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    /// C++ `void printReading(const Reading* reading, std::ostream& output,
    /// size_t sub = 1)`. Resolves `Reading*`/`Cohort*` through `store`.
    pub fn print_reading<W: Write>(
        &self,
        store: &mut RuntimeStore,
        reading: ReadingId,
        output: &mut W,
        sub: usize,
    ) {
        let (noprint, deleted, baseform, parent_cid) = {
            let r = store.readings.get(reading.0);
            (r.noprint, r.deleted, r.baseform, r.parent)
        };
        if noprint {
            return;
        }
        if deleted {
            if !self.trace {
                return;
            }
            u_fputc(';', output);
        }
        for _ in 0..sub {
            u_fputc('\t', output);
        }
        let parent_cid = parent_cid.expect("reading has no parent cohort");
        let wordform_hash = {
            let wf = store.cohorts.get(parent_cid.0).wordform;
            wf.map(|t| self.grammar.single_tags_list[t.0].hash).unwrap_or(0)
        };

        if baseform != 0 {
            let tid = tag_by_hash(&self.grammar, baseform);
            let _ = write!(output, "{}", self.grammar.single_tags_list[tid.0].tag);
        }

        let tags_list: Vec<u32> = store.readings.get(reading.0).tags_list.clone();
        let mut unique: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        let mut mappings: Vec<TagId> = Vec::new();
        for tter in tags_list {
            if (!self.show_end_tags && tter == self.endtag) || tter == self.begintag {
                continue;
            }
            if tter == baseform || tter == wordform_hash {
                continue;
            }
            if self.unique_tags {
                if unique.contains(&tter) {
                    continue;
                }
                unique.insert(tter);
            }
            let tid = tag_by_hash(&self.grammar, tter);
            let ttype = self.grammar.single_tags_list[tid.0].r#type;
            if ttype.intersects(T_DEPENDENCY) && self.has_dep && !self.dep_original {
                continue;
            }
            if ttype.intersects(T_RELATION) && self.has_relations {
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
        let parent_removed = store.cohorts.get(parent_cid.0).r#type.intersects(CT_REMOVED);
        if self.has_dep && !parent_removed {
            {
                let c = store.cohorts.get_mut(parent_cid.0);
                if c.dep_self == 0 {
                    c.dep_self = c.global_number;
                }
            }
            let (p_global, p_local, p_dep_parent, p_sw) = {
                let c = store.cohorts.get(parent_cid.0);
                (c.global_number, c.local_number, c.dep_parent, c.parent)
            };
            let mut pr = parent_cid;
            if p_dep_parent != DEP_NO_PARENT {
                if p_dep_parent == 0 {
                    // parent->parent->cohorts[0]
                    if let Some(sw) = p_sw {
                        pr = store.single_windows.get(sw.0).cohorts[0];
                    }
                } else if let Some(&mapped) = self.gWindow.cohort_map.get(&p_dep_parent) {
                    pr = mapped;
                }
            }
            let arrow = if self.unicode_tags { "\u{2192}" } else { "->" };
            if self.dep_absolute {
                let pr_global = store.cohorts.get(pr.0).global_number;
                let _ = write!(output, " #{p_global}{arrow}{pr_global}");
            } else if !self.dep_has_spanned {
                let pr_local = store.cohorts.get(pr.0).local_number;
                let _ = write!(output, " #{p_local}{arrow}{pr_local}");
            } else {
                let w = self.dep_span_width();
                let p_win = p_sw.map(|s| store.single_windows.get(s.0).number).unwrap_or(0);
                if p_dep_parent == DEP_NO_PARENT {
                    let _ = write!(output, " #{a}{b:0w$}{arrow}{c}{d:0w$}",
                            a = p_win,
                            b = p_local,
                            c = p_win,
                            d = p_local,
                            w = w);
                } else {
                    let (pr_local, pr_win) = {
                        let c = store.cohorts.get(pr.0);
                        let win = c.parent.map(|s| store.single_windows.get(s.0).number).unwrap_or(0);
                        (c.local_number, win)
                    };
                    let _ = write!(output, " #{a}{b:0w$}{arrow}{c}{d:0w$}",
                            a = p_win,
                            b = p_local,
                            c = pr_win,
                            d = pr_local,
                            w = w);
                }
            }
        }

        // --- ID + relations ---
        let (p_related, p_global2, relations) = {
            let c = store.cohorts.get(parent_cid.0);
            (c.r#type.intersects(CT_RELATED), c.global_number, c.relations.clone())
        };
        if self.print_ids || p_related {
            let _ = write!(output, " ID:{p_global2}");
            for (rel_hash, targets) in relations.iter() {
                for siter in targets.iter().copied() {
                    let tid = tag_by_hash(&self.grammar, *rel_hash);
                    let _ = write!(output, " R:{}:{siter}", self.grammar.single_tags_list[tid.0].tag);
                }
            }
        }

        if self.trace {
            let hit_by: Vec<u32> = store.readings.get(reading.0).hit_by.clone();
            for hb in hit_by {
                u_fputc(' ', output);
                self.print_trace(output, hb);
            }
        }

        u_fputc('\n', output);

        let next = store.readings.get(reading.0).next;
        if let Some(next_id) = next {
            store.readings.get_mut(next_id.0).deleted = deleted;
            self.print_reading(store, next_id, output, sub + 1);
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
    /// C++ `virtual void printCohort(Cohort* cohort, std::ostream& output,
    /// bool profiling = false)`.
    pub fn print_cohort<W: Write>(
        &mut self,
        store: &mut RuntimeStore,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        let local_number = store.cohorts.get(cohort.0).local_number;
        // `goto removed` from local_number == 0 skips the entire main body.
        if local_number != 0 {
            if profiling && Some(cohort) == self.rule_target {
                let _ = write!(output, "# RULE TARGET BEGIN\n");
            }

            let wblank = store.cohorts.get(cohort.0).wblank.clone();
            if !wblank.is_empty() {
                self.print_plain_text_line(&wblank, output);
                if !isnl(wblank.chars().next_back().unwrap_or('\0')) {
                    u_fputc('\n', output);
                }
            }

            let mut removed_goto = false;
            if store.cohorts.get(cohort.0).r#type.intersects(CT_REMOVED) {
                if !self.trace || self.trace_no_removed {
                    removed_goto = true;
                } else {
                    u_fputc(';', output);
                    u_fputc(' ', output);
                }
            }

            if !removed_goto {
                let (wf_tag, wf_hash) = {
                    let wf = store.cohorts.get(cohort.0).wordform.expect("cohort wordform");
                    let t = &self.grammar.single_tags_list[wf.0];
                    (t.tag.clone(), t.hash)
                };
                let _ = write!(output, "{wf_tag}");
                if let Some(wr) = store.cohorts.get(cohort.0).wread {
                    let tags: Vec<u32> = store.readings.get(wr.0).tags_list.clone();
                    for tter in tags {
                        if tter == wf_hash {
                            continue;
                        }
                        let tid = tag_by_hash(&self.grammar, tter);
                        let _ = write!(output, " {}", self.grammar.single_tags_list[tid.0].tag);
                    }
                }
                u_fputc('\n', output);

                if !profiling {
                    unignore_all(store, cohort);
                    if !self.split_mappings {
                        // merge_mappings reads self.store; the live store is the
                        // param here (self.store is empty during the caller's
                        // mem::take swap), so swap it in around the call.
                        std::mem::swap(&mut self.store, store);
                        self.merge_mappings(cohort);
                        std::mem::swap(&mut self.store, store);
                    }
                }

                // std::sort(readings, cmp_number)
                let mut readings: Vec<ReadingId> = store.cohorts.get(cohort.0).readings.clone();
                sort_readings(store, &mut readings);
                store.cohorts.get_mut(cohort.0).readings = readings.clone();
                for r in readings {
                    self.print_reading(store, r, output, 1);
                }

                if self.trace && !self.trace_no_removed {
                    let mut delayed: Vec<ReadingId> = store.cohorts.get(cohort.0).delayed.clone();
                    sort_readings(store, &mut delayed);
                    store.cohorts.get_mut(cohort.0).delayed = delayed.clone();
                    for r in delayed {
                        self.print_reading(store, r, output, 1);
                    }
                    let mut del: Vec<ReadingId> = store.cohorts.get(cohort.0).deleted.clone();
                    sort_readings(store, &mut del);
                    store.cohorts.get_mut(cohort.0).deleted = del.clone();
                    for r in del {
                        self.print_reading(store, r, output, 1);
                    }
                }
            }
        }

        // removed:
        let text = store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() && text.chars().any(|c| !self.is_ws(c)) {
            self.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        if profiling && Some(cohort) == self.rule_target {
            let _ = write!(output, "# RULE TARGET END\n");
        }
    }

    /// C++ `UString::find_first_not_of(ws)` membership: is `c` in the (NUL-
    /// terminated) whitespace set `ws`?
    fn is_ws(&self, c: char) -> bool {
        for &w in &self.ws {
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
    pub fn print_single_window<W: Write>(
        &mut self,
        store: &mut RuntimeStore,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        // (The C++ virtual dispatch to the MweSplit / FormatConverter
        // overrides is the StreamFormat strategy; this is the base CG
        // implementation.)
        let (vars_output, all_cohorts, text, text_post, flush_after) = {
            let w = store.single_windows.get(window.0);
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
                let tid = tag_by_hash(&self.grammar, var);
                self.grammar.single_tags_list[tid.0].tag.clone()
            };
            let value_hash: Option<u32> = {
                let w = store.single_windows.get(window.0);
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
                        let vtid = tag_by_hash(&self.grammar, vh);
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
            self.print_cohort(store, cohort, output, profiling);
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
    pub fn pipe_out_reading<W: Write>(
        &self,
        store: &RuntimeStore,
        reading: ReadingId,
        output: &mut W,
    ) {
        let mut ss: Vec<u8> = Vec::new();

        let r = store.readings.get(reading.0);
        let mut flags: u32 = 0;
        if r.noprint {
            flags |= 1 << 1;
        }
        if r.deleted {
            flags |= 1 << 2;
        }
        if r.baseform != 0 {
            flags |= 1 << 3;
        }
        write_raw(&mut ss, flags);

        if r.baseform != 0 {
            let tid = tag_by_hash(&self.grammar, r.baseform);
            write_utf8_raw(&mut ss, &self.grammar.single_tags_list[tid.0].tag);
        }

        let wordform_hash = store
            .cohorts
            .get(r.parent.expect("reading parent").0)
            .wordform
            .map(|t| self.grammar.single_tags_list[t.0].hash)
            .unwrap_or(0);

        let mut cs: u32 = 0;
        for &tter in &r.tags_list {
            if tter == r.baseform || tter == wordform_hash {
                continue;
            }
            let tid = tag_by_hash(&self.grammar, tter);
            if self.grammar.single_tags_list[tid.0].r#type.intersects(T_DEPENDENCY) && self.has_dep {
                continue;
            }
            cs += 1;
        }
        write_raw(&mut ss, cs);
        for &tter in &r.tags_list {
            if tter == r.baseform || tter == wordform_hash {
                continue;
            }
            let tid = tag_by_hash(&self.grammar, tter);
            if self.grammar.single_tags_list[tid.0].r#type.intersects(T_DEPENDENCY) && self.has_dep {
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
    pub fn pipe_out_cohort<W: Write>(
        &self,
        store: &RuntimeStore,
        cohort: CohortId,
        output: &mut W,
    ) {
        let mut ss: Vec<u8> = Vec::new();

        let c = store.cohorts.get(cohort.0);
        write_raw(&mut ss, c.global_number);

        let mut flags: u32 = 0;
        if !c.text.is_empty() {
            flags |= 1 << 0;
        }
        if self.has_dep && c.dep_parent != DEP_NO_PARENT {
            flags |= 1 << 1;
        }
        write_raw(&mut ss, flags);

        if self.has_dep && c.dep_parent != DEP_NO_PARENT {
            write_raw(&mut ss, c.dep_parent);
        }

        let wf = c.wordform.expect("cohort wordform");
        write_utf8_raw(&mut ss, &self.grammar.single_tags_list[wf.0].tag);

        let cs = ui32(c.readings.len());
        write_raw(&mut ss, cs);
        let readings: Vec<ReadingId> = c.readings.clone();
        let text = c.text.clone();
        for rter1 in readings {
            self.pipe_out_reading(store, rter1, &mut ss);
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
    pub fn pipe_out_single_window(
        &self,
        store: &RuntimeStore,
        window: SwId,
        output: &mut Process,
    ) {
        let mut ss: Vec<u8> = Vec::new();

        let (number, cohorts) = {
            let w = store.single_windows.get(window.0);
            (w.number, w.cohorts.clone())
        };
        write_raw(&mut ss, number);

        let cs = ui32(cohorts.len()) - 1;
        write_raw(&mut ss, cs);

        for c in 1..(cs + 1) {
            self.pipe_out_cohort(store, cohorts[c as usize], &mut ss);
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
    pub fn pipe_in_reading(
        &mut self,
        store: &mut RuntimeStore,
        reading: ReadingId,
        input: &mut Process,
        force: bool,
    ) {
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
            let r = store.readings.get_mut(reading.0);
            r.noprint = (flags & (1 << 1)) != 0;
            r.deleted = (flags & (1 << 2)) != 0;
        }

        if flags & (1 << 3) != 0 {
            let str = read_utf8_raw(&mut ss);
            let baseform = store.readings.get(reading.0).baseform;
            let cur = {
                let tid = tag_by_hash(&self.grammar, baseform);
                self.grammar.single_tags_list[tid.0].tag.clone()
            };
            if str != cur {
                let tag = self.add_tag(&str, crate::tag::TagType::empty());
                store.readings.get_mut(reading.0).baseform =
                    self.grammar.single_tags_list[tag.0].hash;
            }
        } else {
            store.readings.get_mut(reading.0).baseform = 0;
        }

        let (wordform_hash, baseform) = {
            let r = store.readings.get(reading.0);
            let wf = store
                .cohorts
                .get(r.parent.expect("reading parent").0)
                .wordform
                .map(|t| self.grammar.single_tags_list[t.0].hash)
                .unwrap_or(0);
            (wf, r.baseform)
        };
        {
            let r = store.readings.get_mut(reading.0);
            r.tags_list.clear();
            r.tags_list.push(wordform_hash);
            if baseform != 0 {
                r.tags_list.push(baseform);
            }
        }

        let cs: u32 = read_raw(&mut ss);
        for _ in 0..cs {
            let str = read_utf8_raw(&mut ss);
            let tag = self.add_tag(&str, crate::tag::TagType::empty());
            let hash = self.grammar.single_tags_list[tag.0].hash;
            store.readings.get_mut(reading.0).tags_list.push(hash);
        }

        // reflowReading(*reading); — reflow_reading reads self.store, but the
        // caller has lifted the store out of self (mem::take) to hand it to the
        // pipe_* fns; swap the real store back in around the call.
        std::mem::swap(&mut self.store, store);
        self.reflow_reading(reading);
        std::mem::swap(&mut self.store, store);
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
    /// C++ `void pipeInCohort(Cohort* cohort, Process& input)`.
    pub fn pipe_in_cohort(&mut self, store: &mut RuntimeStore, cohort: CohortId, input: &mut Process) {
        let _packet_len: u32 = read_raw(&mut ProcRead(input));

        let cs: u32 = read_raw(&mut ProcRead(input));
        let global_number = store.cohorts.get(cohort.0).global_number;
        if cs != global_number {
            // "Error: External returned data for cohort ... but we expected ...!"
            cg3_quit(1, Some(file!()), self.numLines);
        }

        let flags: u32 = read_raw(&mut ProcRead(input));

        if flags & (1 << 1) != 0 {
            let dp: u32 = read_raw(&mut ProcRead(input));
            store.cohorts.get_mut(cohort.0).dep_parent = dp;
        }

        let mut force_readings = false;
        let str = read_utf8_raw(&mut ProcRead(input));
        let cur_wf = store
            .cohorts
            .get(cohort.0)
            .wordform
            .map(|t| self.grammar.single_tags_list[t.0].tag.clone())
            .unwrap_or_default();
        if str != cur_wf {
            let tag = self.add_tag(&str, crate::tag::TagType::empty());
            store.cohorts.get_mut(cohort.0).wordform = Some(tag);
            force_readings = true;
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        for i in 0..cs {
            let rid = store.cohorts.get(cohort.0).readings[i as usize];
            self.pipe_in_reading(store, rid, input, force_readings);
        }

        if flags & (1 << 0) != 0 {
            let text = read_utf8_raw(&mut ProcRead(input));
            store.cohorts.get_mut(cohort.0).text = text;
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
    /// C++ `void pipeInSingleWindow(SingleWindow& window, Process& input)`.
    pub fn pipe_in_single_window(&mut self, store: &mut RuntimeStore, window: SwId, input: &mut Process) {
        let cs: u32 = read_raw(&mut ProcRead(input));
        if cs == 0 {
            return;
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        let number = store.single_windows.get(window.0).number;
        if cs != number {
            // "Error: External returned data for window ... but we expected ...!"
            cg3_quit(1, Some(file!()), self.numLines);
        }

        let cs: u32 = read_raw(&mut ProcRead(input));
        for i in 0..cs {
            let cid = store.single_windows.get(window.0).cohorts[(i + 1) as usize];
            self.pipe_in_cohort(store, cid, input);
        }
    }

    // =======================================================================
    // error (4 C++ overloads -> 3 Rust fns; two UChar*/char* single-arg
    // overloads collapse to &str)
    // =======================================================================

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
    pub fn error_ss(&self, _str: &str, _s: &str, _big_s: &str, _p: Option<&str>) -> (&'static str, u32) {
        self.error_labels()
    }

    /// Shared label/line selection for the `error(...)` family: `("RT RULE",
    /// current_rule->line)` when a current rule with a non-zero line is set,
    /// else `("RT INPUT", numLines)`.
    fn error_labels(&self) -> (&'static str, u32) {
        if let Some(rid) = self.current_rule {
            let line = self.grammar.rule_by_number[rid.0].line;
            if line != 0 {
                return ("RT RULE", line);
            }
        }
        ("RT INPUT", self.numLines)
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
    pub fn set_options(&mut self, options: &options_t) {
        let occ = |o: OPTIONS| options[o as usize].does_occur;
        let val = |o: OPTIONS| options[o as usize].value.as_str();

        if occ(OPTIONS::ALWAYS_SPAN) {
            self.always_span = true;
        }
        self.unicode_tags = false;
        if occ(OPTIONS::UNICODE_TAGS) {
            self.unicode_tags = true;
        }
        self.unique_tags = false;
        if occ(OPTIONS::UNIQUE_TAGS) {
            self.unique_tags = true;
        }
        self.apply_mappings = true;
        if occ(OPTIONS::NOMAPPINGS) {
            self.apply_mappings = false;
        }
        self.apply_corrections = true;
        if occ(OPTIONS::NOCORRECTIONS) {
            self.apply_corrections = false;
        }
        self.no_before_sections = false;
        if occ(OPTIONS::NOBEFORESECTIONS) {
            self.no_before_sections = true;
        }
        self.no_sections = false;
        if occ(OPTIONS::NOSECTIONS) {
            self.no_sections = true;
        }
        self.no_after_sections = false;
        if occ(OPTIONS::NOAFTERSECTIONS) {
            self.no_after_sections = true;
        }
        self.r#unsafe = false;
        if occ(OPTIONS::UNSAFE) {
            self.r#unsafe = true;
        }
        if occ(OPTIONS::ORDERED) {
            self.ordered = true;
        }
        if occ(OPTIONS::TRACE) {
            self.trace = true;
            if !val(OPTIONS::TRACE).is_empty() {
                self.set_opts_ranged_interval(val(OPTIONS::TRACE), true, false);
            }
        }
        if occ(OPTIONS::TRACE_NAME_ONLY) {
            self.trace = true;
            self.trace_name_only = true;
        }
        if occ(OPTIONS::TRACE_NO_REMOVED) {
            self.trace = true;
            self.trace_no_removed = true;
        }
        if occ(OPTIONS::TRACE_ENCL) {
            self.trace = true;
            self.trace_encl = true;
        }
        if occ(OPTIONS::PIPE_DELETED) {
            self.pipe_deleted = true;
        }
        if occ(OPTIONS::DRYRUN) {
            self.dry_run = true;
        }
        if occ(OPTIONS::SINGLERUN) {
            self.section_max_count = 1;
        }
        if occ(OPTIONS::MAXRUNS) {
            self.section_max_count = stoul(val(OPTIONS::MAXRUNS));
        }
        if occ(OPTIONS::SECTIONS) {
            g_app_set_opts_ranged(val(OPTIONS::SECTIONS), &mut self.sections, true);
        }
        if occ(OPTIONS::RULES) {
            self.set_opts_ranged_interval_valid(val(OPTIONS::RULES), true);
        }
        if occ(OPTIONS::RULE) {
            let v = val(OPTIONS::RULE);
            let first = v.chars().next().unwrap_or('\0');
            if first.is_ascii_digit() {
                self.valid_rules.insert_sorted(stoi(v) as u32);
            } else {
                // ucnv_toUChars is identity for UTF-8; compare rule names.
                for i in 0..self.grammar.rule_by_number.capacity() {
                    if let Some(rule) = self.grammar.rule_by_number.try_get(i) {
                        if rule.name == v {
                            self.valid_rules.insert_sorted(rule.number);
                        }
                    }
                }
            }
        }
        if occ(OPTIONS::DEBUG_RULES) {
            self.set_opts_ranged_interval_debug(val(OPTIONS::DEBUG_RULES), false);
        }
        if occ(OPTIONS::VERBOSE) {
            self.verbosity_level = if !val(OPTIONS::VERBOSE).is_empty() {
                stoul(val(OPTIONS::VERBOSE))
            } else {
                1
            };
        }
        if occ(OPTIONS::DODEBUG) {
            self.debug_level = if !val(OPTIONS::DODEBUG).is_empty() {
                stoul(val(OPTIONS::DODEBUG))
            } else {
                1
            };
            // C++ `std::cerr << "Debug level set to " << debug_level`: deferred.
        }
        if occ(OPTIONS::PRINT_IDS) {
            self.print_ids = true;
        }
        if occ(OPTIONS::PRINT_DEP) {
            self.has_dep = true;
        }
        if occ(OPTIONS::NUM_WINDOWS) {
            self.num_windows = stoul(val(OPTIONS::NUM_WINDOWS));
        }
        if occ(OPTIONS::SOFT_LIMIT) {
            self.soft_limit = stoul(val(OPTIONS::SOFT_LIMIT));
        }
        if occ(OPTIONS::HARD_LIMIT) {
            self.hard_limit = stoul(val(OPTIONS::HARD_LIMIT));
        }
        if occ(OPTIONS::TEXT_DELIMIT) {
            let rx = if !val(OPTIONS::TEXT_DELIMIT).is_empty() {
                val(OPTIONS::TEXT_DELIMIT).to_string()
            } else {
                STR_TEXTDELIM_DEFAULT.to_string()
            };
            self.set_text_delimiter(rx);
        }
        if occ(OPTIONS::DEP_DELIMIT) {
            self.dep_delimit = if !val(OPTIONS::DEP_DELIMIT).is_empty() {
                stoul(val(OPTIONS::DEP_DELIMIT))
            } else {
                10
            };
            self.parse_dep = true;
        }
        if occ(OPTIONS::DEP_ABSOLUTE) {
            self.dep_absolute = true;
        }
        if occ(OPTIONS::DEP_ORIGINAL) {
            self.dep_original = true;
        }
        if occ(OPTIONS::DEP_ALLOW_LOOPS) {
            self.dep_block_loops = false;
        }
        if occ(OPTIONS::DEP_BLOCK_CROSSING) {
            self.dep_block_crossing = true;
        }
        if occ(OPTIONS::MAGIC_READINGS) {
            self.allow_magic_readings = false;
        }
        if occ(OPTIONS::NO_PASS_ORIGIN) {
            self.no_pass_origin = true;
        }
        if occ(OPTIONS::SPLIT_MAPPINGS) {
            self.split_mappings = true;
        }
        if occ(OPTIONS::SHOW_END_TAGS) {
            self.show_end_tags = true;
        }
        if occ(OPTIONS::NO_BREAK) {
            self.add_spacing = false;
        }
    }

    /// `GAppSetOpts_ranged(value, trace_rules, fill)` bridge: the ported helper
    /// fills a `Vec<u32>`, so expand there and insert into the interval vector.
    fn set_opts_ranged_interval(&mut self, value: &str, _default_true: bool, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.trace_rules.insert(v);
        }
    }
    fn set_opts_ranged_interval_valid(&mut self, value: &str, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.valid_rules.insert(v);
        }
    }
    fn set_opts_ranged_interval_debug(&mut self, value: &str, fill: bool) {
        let mut tmp: Vec<u32> = Vec::new();
        g_app_set_opts_ranged(value, &mut tmp, fill);
        for v in tmp {
            self.debug_rules.insert(v);
        }
    }

    // =======================================================================
    // printDebugRule / addProfilingExample / profileRuleContext (inline .hpp)
    // =======================================================================

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]
    /// C++ inline `void printDebugRule(const Rule& rule, bool target, bool cntx)`.
    /// Renders the whole in-flight window set (profiling mode) with `trace`
    /// force-disabled, into a buffer written to stderr (the C++ `ux_stderr`);
    /// the C++ `swapper<bool>` is a manual save/restore of `trace`.
    pub fn print_debug_rule(&mut self, store: &mut RuntimeStore, rule: RuleId, target: bool, cntx: bool) {
        let saved_trace = self.trace;
        self.trace = false; // swapper<bool>(true, trace, ttrace=false)

        let mut buf: Vec<u8> = Vec::new();
        let line = self.grammar.rule_by_number[rule.0].line;
        let _ = write!(&mut buf, "# ===== BEGIN RULE {}{}{} =====\n",
                line,
                if target { " TARGET-MATCH" } else { " TARGET-FAIL" },
                if cntx { " CONTEXT-MATCH" } else { " CONTEXT-FAIL" });

        let _ = write!(&mut buf, "# PREVIOUS WINDOWS\n");
        for s in self.gWindow.previous.clone() {
            self.print_single_window(store, s, &mut buf, true);
        }
        let _ = write!(&mut buf, "# CURRENT WINDOW\n");
        if let Some(cur) = self.gWindow.current {
            self.print_single_window(store, cur, &mut buf, true);
        }
        let _ = write!(&mut buf, "# NEXT WINDOWS\n");
        for s in self.gWindow.next.clone() {
            self.print_single_window(store, s, &mut buf, true);
        }

        let _ = write!(&mut buf, "# ===== END RULE {line} =====\n");

        // u_fprintf(ux_stderr, "%s", buf) — a raw stream dump (window data),
        // not a log event: write it straight to stderr like the C++.
        let _ = std::io::stderr().write_all(&buf);
        self.trace = saved_trace;
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]
    /// C++ template `void addProfilingExample(T& item)`. Renders the whole
    /// in-flight window set (previous / current / next, trace force-disabled via
    /// the C++ `swapper<bool>`) into a buffer, interns it in the profiler string
    /// table, and stores the id into `entries[key].example_window`. (The C++
    /// passes the entry by reference; the port passes its `key` — same entry,
    /// borrow-checker-friendly.) Caller guarantees `self.profiler` is `Some` and
    /// the entry exists.
    pub(super) fn add_profiling_example(&mut self, store: &mut RuntimeStore, key: crate::profiler::Key) {
        let saved_trace = self.trace;
        self.trace = false; // swapper<bool> _st(true, trace, ttrace=false)

        let mut buf: Vec<u8> = Vec::new();
        let _ = write!(&mut buf, "# PREVIOUS WINDOWS\n");
        for s in self.gWindow.previous.clone() {
            self.print_single_window(store, s, &mut buf, true);
        }
        let _ = write!(&mut buf, "# CURRENT WINDOW\n");
        if let Some(cur) = self.gWindow.current {
            self.print_single_window(store, cur, &mut buf, true);
        }
        let _ = write!(&mut buf, "# NEXT WINDOWS\n");
        for s in self.gWindow.next.clone() {
            self.print_single_window(store, s, &mut buf, true);
        }
        self.trace = saved_trace;

        let p = self.profiler.as_mut().unwrap();
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
        if self.profiler.is_none() {
            return;
        }
        let test_hash = self.grammar.contexts_arena[test.0].hash;
        let test_pos = self.grammar.contexts_arena[test.0].pos;
        let rule_number = self.grammar.rule_by_number.get(rule.0).number;
        let key = crate::profiler::Key { r#type: crate::profiler::ET_CONTEXT, id: test_hash };
        let p = self.profiler.as_mut().unwrap();
        let Some(t) = p.entries.get_mut(&key) else {
            return;
        };
        if (test_good && (!test_pos.intersects(POS_NEGATE))) || (!test_good && (test_pos.intersects(POS_NEGATE))) {
            t.num_match += 1;
            let need_example = t.example_window == 0;
            *p.rule_contexts.entry((rule_number + 1, test_hash)).or_insert(0) += 1;
            if need_example {
                // print_single_window needs `store` distinct from `&mut self`.
                let mut store = std::mem::take(&mut self.store);
                self.add_profiling_example(&mut store, key);
                self.store = store;
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

/// The applicator instantiation of the C++ `parser_helpers.hpp`
/// `template<typename State> parseTag(...)` — used by
/// [`GrammarApplicator::add_tag`]'s `T_VARSTRING` branch so runtime-generated
/// tags go through the full parser (regex compile, prefixes, suffixes,
/// numerics) instead of the raw path.
impl crate::parser_helpers::ParseTagState for super::GrammarApplicator {
    fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    /// C++ `GrammarApplicator::filebase` is `nullptr` (never set) — the
    /// warnings that print it only fire on malformed tags.
    fn filebase(&self) -> &str {
        ""
    }

    /// C++ `GrammarApplicator::error(str, p)` prints `("RT RULE",
    /// current_rule->line)` / `("RT INPUT", numLines)` into the format and
    /// RETURNS (non-fatal, unlike `TextualParser::error`). The port's
    /// `error()` defers the sink, so emit a plain stderr line here.
    fn error_near(&mut self, _near: &[char]) {
        let (label, line) = self.error("", None);
        tracing::error!("Error: parseTag failed at {label} {line}");
    }

    /// C++ `state.addTag(tag)` → `GrammarApplicator::addTag(Tag*)` — the
    /// seed-probing interner, NOT `Grammar::addTag`.
    fn add_tag(&mut self, tag: Tag) -> TagId {
        self.add_tag_ptr(tag)
    }
}
