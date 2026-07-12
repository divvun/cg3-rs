//! Port of `src/MatxinApplicator.{cpp,hpp}` — the Matxin XML dependency-tree I/O
//! applicator. Parses an Apertium-style stream and emits a Matxin `<corpus>` of
//! `<SENTENCE>` blocks with nested `<NODE>` dependency trees.
//!
//! COMPOSITION-OVER-INHERITANCE. C++ `class MatxinApplicator : public virtual
//! GrammarApplicator`. In Rust the base engine is held by value in `base`; all
//! engine/core calls go through `self.base.<method>` / `self.base.store` /
//! `self.base.grammar` / `self.base.gWindow`.
//!
//! ARENA MODEL. Pointers become arena ids resolved through `self.base.store`
//! (`Cohort*`→`CohortId`, `Reading*`→`ReadingId`, `SingleWindow*`→`SwId`) and
//! `self.base.grammar.single_tags_list` (`Tag*`→`TagId`). Char-by-char C++ walks
//! (UTF-16 `UChar`) become UTF-8 char reads via `uextras::u_fgetc`.
//!
//! OUTPUT SINK. C++ `std::ostream& output` → generic `output: &mut W`
//! (`W: std::io::Write`).
//!
//! REPRODUCED BUGS (bug-for-bug):
//! * XML-escape entity+literal bug in `print_single_window`: after emitting
//!   `&amp;`/`&quot;` for `&`/`"`, the raw char is UNCONDITIONALLY appended too,
//!   so `&`→`&amp;&` and `"`→`&quot;"`.
//! * `nodes`/`deps` member maps are NEVER cleared between windows, so successive
//!   `<SENTENCE>` blocks accumulate/cross-contaminate node/dependency state; the
//!   "last word" fallback (`nodes.len()`) also grows across windows.
//! * `print_reading` hard-exits (`std::process::exit(-1)`) when a reading has
//!   sub-readings (Matxin cannot represent them).

use std::collections::BTreeMap;
use std::io::Write;

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::{CT_REMOVED, alloc_cohort, append_reading, unignore_all};
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{hash_value, insert_if_exists};
use crate::reading::{Reading, ReadingList, alloc_reading, alloc_reading_copy};
use crate::single_window::append_cohort;
use crate::store::RuntimeStore;
use crate::tag::{T_BASEFORM, T_MAPPING, T_WORDFORM, TagVector};
use crate::types::{TagHash, UString};
use crate::uextras::{U_EOF, u_fflush, u_fgetc, u_fputc, ux_strip_bom};

// C++ `Strings.hpp` string constants.
const STR_BEGINTAG: &str = ">>>";
const STR_ENDTAG: &str = "<<<";

// [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.node]
/// C++ nested `struct Node { int self; UString lemma; form; pos; mi; si; }`.
#[derive(Default, Clone)]
pub struct Node {
    pub self_: i32,
    pub lemma: UString,
    pub form: UString,
    pub pos: UString,
    pub mi: UString,
    pub si: UString,
}

// [spec:cg3:def:matxin-applicator.cg3.matxin-applicator]
/// C++ `class MatxinApplicator : public virtual GrammarApplicator`.
pub struct MatxinApplicator {
    /// The base engine (C++ inheritance → composition).
    pub base: GrammarApplicator,
    pub wordform_case: bool,
    pub print_word_forms: bool,
    pub print_only_first: bool,
    /// C++ `std::map<int, Node> nodes`.
    pub nodes: BTreeMap<i32, Node>,
    /// C++ `std::map<int, std::vector<int>> deps`.
    pub deps: BTreeMap<i32, Vec<i32>>,
    pub null_flush: bool,
    pub running_with_null_flush: bool,
}

// ---------------------------------------------------------------------------
// Port-infra helpers (un-annotated).
// ---------------------------------------------------------------------------

/// Resolve a tag hash to its `TagId` via `grammar->single_tags[hash]`. Mirrors
/// the engine's private `tag_by_hash` (not visible from this sibling module).
fn tag_by_hash(grammar: &crate::grammar::Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags.find(hash.get());
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

/// C++ `reverse(Reading* head)` specialised to the arena `ReadingId` `next`
/// chain: reverses in place and returns the new head.
fn reverse_reading(store: &mut RuntimeStore, head: ReadingId) -> ReadingId {
    let mut nr: Option<ReadingId> = None;
    let mut cur: Option<ReadingId> = Some(head);
    while let Some(h) = cur {
        let next = store.readings.get(h.0).next;
        store.readings.get_mut(h.0).next = nr;
        nr = Some(h);
        cur = next;
    }
    nr.unwrap_or(head)
}

impl MatxinApplicator {
    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.matxin-applicator-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.matxin-applicator-fn]
    /// C++ `MatxinApplicator::MatxinApplicator(std::ostream& ux_err)` — forwards
    /// `ux_err` to the base ctor (body empty); members keep in-class defaults.
    pub fn new(base: GrammarApplicator) -> Self {
        MatxinApplicator {
            base,
            wordform_case: false,
            print_word_forms: true,
            print_only_first: false,
            nodes: BTreeMap::new(),
            deps: BTreeMap::new(),
            null_flush: false,
            running_with_null_flush: false,
        }
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.get-null-flush-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.get-null-flush-fn]
    /// C++ `bool MatxinApplicator::getNullFlush()` — trivial getter.
    pub fn get_null_flush(&self) -> bool {
        self.null_flush
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.set-null-flush-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.set-null-flush-fn]
    /// C++ `void MatxinApplicator::setNullFlush(bool pNullFlush)` — trivial setter.
    pub fn set_null_flush(&mut self, p_null_flush: bool) {
        self.null_flush = p_null_flush;
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.test-pr-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.test-pr-fn]
    /// C++ `void testPR(std::ostream& output)` — DECLARED-but-never-DEFINED in
    /// `MatxinApplicator.hpp`. There is no body anywhere in the source tree and it
    /// is never called, so it links only because its address is never taken. The
    /// faithful port of this "unimplemented declared method" is a no-op stub; the
    /// real functional-test debug routine lives only in `ApertiumApplicator::testPR`.
    #[allow(dead_code)]
    pub fn test_pr<W: Write>(&self, _output: &mut W) {
        // No C++ definition exists; body intentionally empty.
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.merge-mappings-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.merge-mappings-fn]
    /// C++ `void MatxinApplicator::mergeMappings(Cohort& cohort)`. Like Apertium's
    /// but the survivor is a fresh COPY (`alloc_reading(*(clist.front()))`) — the
    /// originals are NOT freed (orphaned in the pool).
    pub fn merge_mappings(&mut self, cohort: CohortId) {
        let store = &mut self.base.store;
        let trace = self.base.trace;

        let readings: ReadingList = store.cohorts.get(cohort.0).readings.clone();
        let mut mlist: BTreeMap<u32, ReadingList> = BTreeMap::new();
        for &r in &readings {
            let mut hp = store.readings.get(r.0).hash;
            if trace {
                let hb = store.readings.get(r.0).hit_by.clone();
                for iter_hb in hb {
                    hp = hash_value(iter_hb, hp);
                }
            }
            let mut sub = store.readings.get(r.0).next;
            while let Some(s) = sub {
                hp = hash_value(store.readings.get(s.0).hash, hp);
                if trace {
                    let hb = store.readings.get(s.0).hit_by.clone();
                    for iter_hb in hb {
                        hp = hash_value(iter_hb, hp);
                    }
                }
                sub = store.readings.get(s.0).next;
            }
            mlist.entry(hp).or_default().push(r);
        }

        if mlist.len() == readings.len() {
            return;
        }

        store.cohorts.get_mut(cohort.0).readings.clear();
        let mut order: Vec<ReadingId> = Vec::new();
        for (_k, clist) in mlist.into_iter() {
            // alloc_reading(*(clist.front())) — a fresh COPY; originals orphaned.
            let src = clone_reading_value(store.readings.get(clist[0].0));
            let nr = alloc_reading_copy(store, &src);
            order.push(nr);
        }

        order.sort_by(|&a, &b| cmp_reading(store, a, b));
        let c = store.cohorts.get_mut(cohort.0);
        for (i, r) in order.into_iter().enumerate() {
            c.readings.insert(i, r);
        }
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.process-reading-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.process-reading-fn]
    /// C++ `void MatxinApplicator::processReading(Reading* cReading, const UChar*
    /// reading_string)`. Parses one Matxin/Apertium-style analysis string.
    pub fn process_reading(&mut self, c_reading: ReadingId, reading_string: &[char]) {
        let s = reading_string;
        let len = s.len();

        // insert_if_exists(cReading->parent->possible_sets, grammar->sets_any)
        if let Some(parent) = self.base.store.readings.get(c_reading.0).parent {
            insert_if_exists(
                &mut self.base.store.cohorts.get_mut(parent.0).possible_sets,
                self.base.grammar.sets_any.as_ref(),
            );
        }

        // Pass 1: find the multiword suffix `suf`.
        let mut suf: UString = String::new();
        {
            let mut tags = false;
            let mut multi = false;
            let mut m = 0usize;
            while m < len {
                let ch = s[m];
                if ch == '<' {
                    tags = true;
                }
                if ch == '#' && tags {
                    multi = true;
                }
                if ch == '+' && multi {
                    multi = false;
                }
                if multi {
                    suf.push(ch);
                }
                m += 1;
            }
        }

        // Build the baseform `base`, wrapped in `"`.
        let mut base: UString = String::from("\"");
        let mut unknown = false;
        {
            let mut c = 0usize;
            while c < len {
                let ch = s[c];
                if ch == '*' {
                    unknown = true;
                }
                if ch == '<' {
                    break;
                }
                base.push(ch);
                c += 1;
            }
        }
        if !suf.is_empty() {
            base.push_str(&suf);
        }
        base.push('"');

        let tag = self.base.add_tag(&base, crate::tag::TagType::empty());

        if unknown {
            let h = self.base.grammar.single_tags_list.get(tag.0).hash;
            self.base.store.readings.get_mut(c_reading.0).baseform = Some(h);
            self.base.add_tag_to_reading(c_reading, tag);
            return;
        }

        let mut taglist: TagVector = vec![tag];

        // Read the tags.
        let mut tmptag: UString = String::new();
        let mut joiner = false;
        let mut intag = false;
        let mut multi = false;
        {
            // c must resume from the start (C++ re-walks `c` from the beginning).
            let mut c = 0usize;
            while c < len {
                let ch = s[c];
                if ch == '+' {
                    multi = false;
                    joiner = true;
                }
                if ch == '#' && !intag {
                    multi = true;
                }
                if ch == '<' {
                    multi = false;
                    if intag {
                        tracing::error!(
                            "Error: The Matxin stream format does not allow '<' in tag names."
                        );
                        c += 1;
                        continue;
                    }
                    intag = true;
                    if joiner {
                        // Flush the pending joined baseform.
                        let mut bf: UString = String::from("\"");
                        let tt: Vec<char> = tmptag.chars().collect();
                        if tt.first() == Some(&'+') {
                            bf.extend(tt[1..].iter());
                        } else {
                            bf.push_str(&tmptag);
                        }
                        bf.push('"');
                        let t = self.base.add_tag(&bf, crate::tag::TagType::empty());
                        taglist.push(t);
                        tmptag.clear();
                        joiner = false;
                    }
                    c += 1;
                    continue;
                } else if ch == '>' {
                    multi = false;
                    if !intag {
                        tracing::error!(
                            "Error: The Matxin stream format does not allow '>' outside tag names."
                        );
                        c += 1;
                        continue;
                    }
                    intag = false;
                    let t = self.base.add_tag(&tmptag, crate::tag::TagType::empty());
                    taglist.push(t);
                    tmptag.clear();
                    joiner = false;
                    c += 1;
                    continue;
                }
                if multi {
                    c += 1;
                    continue;
                }
                tmptag.push(ch);
                c += 1;
            }
        }

        // Assign tags to reading(s): back-to-front baseform scan.
        while !taglist.is_empty() {
            let mut reading = c_reading;
            let mut ri = taglist.len();
            while ri > 0 {
                ri -= 1;
                let riter = taglist[ri];
                if self
                    .base
                    .grammar
                    .single_tags_list
                    .get(riter.0)
                    .r#type
                    .intersects(T_BASEFORM)
                {
                    if self.base.store.readings.get(reading.0).baseform.is_some() {
                        // Sub-reading — NOTE: Matxin does NOT re-add the wordform.
                        let parent = self.base.store.readings.get(reading.0).parent;
                        let nr = Reading::allocate_reading(&mut self.base.store, parent);
                        self.base.store.readings.get_mut(reading.0).next = Some(nr);
                        reading = nr;
                    }
                    let mut mappings: TagVector = Vec::new();
                    let mprefix = self.base.grammar.mapping_prefix;
                    for k in ri..taglist.len() {
                        let iter = taglist[k];
                        let t = self.base.grammar.single_tags_list.get(iter.0);
                        let is_mapping =
                            t.r#type.intersects(T_MAPPING) || t.tag.chars().next() == Some(mprefix);
                        if is_mapping {
                            mappings.push(iter);
                        } else {
                            self.base.add_tag_to_reading(reading, iter);
                        }
                    }
                    if !mappings.is_empty() {
                        let parent = self.base.store.readings.get(reading.0).parent.unwrap();
                        self.base
                            .split_mappings(&mut mappings, parent, reading, true);
                    }
                    while let Some(&last) = taglist.last() {
                        if self
                            .base
                            .grammar
                            .single_tags_list
                            .get(last.0)
                            .r#type
                            .intersects(T_BASEFORM)
                        {
                            break;
                        }
                        taglist.pop();
                    }
                    taglist.pop();
                    break;
                }
            }
        }
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.print-reading-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-reading-fn]
    /// C++ `void MatxinApplicator::printReading(Reading* reading, Node& node,
    /// std::ostream& output)`. Fills `node` from the reading; hard-exits on
    /// sub-readings (Matxin can't represent them).
    pub fn print_reading<W: Write>(&self, reading: ReadingId, node: &mut Node, output: &mut W) {
        let r = self.base.store.readings.get(reading.0);
        if r.noprint {
            return;
        }
        if r.next.is_some() {
            tracing::error!("Error: input contains sub-readings!");
            let _ = write!(output, "  </SENTENCE>\n");
            let _ = write!(output, "</corpus>\n");
            // C++ exit(-1); wave 4: Cg3Exit unwind (bins convert to the exit).
            crate::error::cg3_exit(-1);
        }
        if r.baseform.is_none() {
            return;
        }

        // Lop off the surrounding '"' quotes.
        let tid = tag_by_hash(&self.base.grammar, r.baseform.unwrap_or(TagHash(0)));
        let tagtext: Vec<char> = self
            .base
            .grammar
            .single_tags_list
            .get(tid.0)
            .tag
            .chars()
            .collect();
        let bf: String = if tagtext.len() >= 2 {
            tagtext[1..tagtext.len() - 1].iter().collect()
        } else {
            String::new()
        };
        node.lemma = bf;

        // Reorder: MAPPING tags before the multiword join.
        let mut tags_list: Vec<u32> = Vec::new();
        let mut multitags_list: Vec<u32> = Vec::new();
        let mut multi = false;
        for &tter in r.tags_list.iter() {
            let tag = self
                .base
                .grammar
                .single_tags_list
                .get(tag_by_hash(&self.base.grammar, TagHash(tter)).0);
            if tag.tag.chars().next() == Some('+') {
                multi = true;
            } else if tag.r#type.intersects(T_MAPPING) {
                multi = false;
            }
            if multi {
                multitags_list.push(tter);
            } else {
                tags_list.push(tter);
            }
        }
        tags_list.extend(multitags_list);

        // Build `mi` (pipe-joined morphology).
        let mut used_tags = crate::sorted_vector::uint32SortedVector::new();
        let mut mi: UString = String::new();
        let mut first = true;
        for tter in tags_list {
            if self.base.unique_tags {
                if used_tags.find(tter) != used_tags.end() {
                    continue;
                }
                used_tags.insert(tter);
            }
            if tter == self.base.endtag.get() || tter == self.base.begintag.get() {
                continue;
            }
            let tag = self
                .base
                .grammar
                .single_tags_list
                .get(tag_by_hash(&self.base.grammar, TagHash(tter)).0);
            if !tag.r#type.intersects(T_BASEFORM) && !tag.r#type.intersects(T_WORDFORM) {
                let firstc = tag.tag.chars().next();
                if firstc == Some('+') {
                    let _ = write!(output, "{}", tag.tag);
                } else if firstc == Some('@') {
                    node.si = tag.tag.clone();
                } else if first {
                    mi.push_str(&tag.tag);
                    first = false;
                } else {
                    mi.push('|');
                    mi.push_str(&tag.tag);
                }
            }
        }
        node.mi = mi;
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.print-single-window-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-single-window-fn]
    /// C++ `void MatxinApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling)`. Emits one `<SENTENCE>` block.
    pub fn print_single_window<W: Write>(&mut self, window: SwId, output: &mut W, profiling: bool) {
        let number = self.base.store.single_windows.get(window.0).number;
        let _ = write!(output, "  <SENTENCE ord=\"{number}\" alloc=\"0\">\n");

        let all_cohorts = self
            .base
            .store
            .single_windows
            .get(window.0)
            .all_cohorts
            .clone();
        for cohort in all_cohorts {
            let (local_number, ctype) = {
                let c = self.base.store.cohorts.get(cohort.0);
                (c.local_number, c.r#type)
            };
            if local_number == 0 || (ctype.intersects(CT_REMOVED)) {
                continue;
            }

            if !profiling {
                unignore_all(&mut self.base.store, cohort);
                if !self.base.split_mappings {
                    self.merge_mappings(cohort);
                }
            }

            let mut n = Node::default();

            // Wordform, `"<`/`>"` stripped, XML-escaped (entity+literal bug).
            let wf_tid = self
                .base
                .store
                .cohorts
                .get(cohort.0)
                .wordform
                .expect("printSingleWindow: cohort has no wordform");
            let wf_chars: Vec<char> = self
                .base
                .grammar
                .single_tags_list
                .get(wf_tid.0)
                .tag
                .chars()
                .collect();
            let wf: Vec<char> = if wf_chars.len() >= 4 {
                wf_chars[2..wf_chars.len() - 2].to_vec()
            } else {
                Vec::new()
            };
            let mut wf_escaped: UString = String::new();
            for &ch in &wf {
                if ch == '&' {
                    wf_escaped.push_str("&amp;");
                } else if ch == '"' {
                    wf_escaped.push_str("&quot;");
                }
                // BUG: the raw char is ALWAYS appended after the entity.
                wf_escaped.push(ch);
            }

            let global_number = self.base.store.cohorts.get(cohort.0).global_number;
            n.self_ = global_number.get() as i32;
            n.form = wf_escaped;

            // Only the FIRST reading.
            let reading = self.base.store.cohorts.get(cohort.0).readings[0];
            self.print_reading(reading, &mut n, output);

            // Fallback root `r`.
            let mut r = self.nodes.len() as i32; // last word
            if let Some(d0) = self.deps.get(&0) {
                if !d0.is_empty() {
                    r = d0[0];
                }
            }

            self.nodes.insert(global_number.get() as i32, n);

            let dep_parent = self.base.store.cohorts.get(cohort.0).dep_parent;
            if dep_parent.is_none() {
                self.deps.entry(r).or_default().push(global_number.get() as i32);
            } else {
                self.deps
                    .entry(dep_parent.unwrap().get() as i32)
                    .or_default()
                    .push(global_number.get() as i32);
            }

            u_fflush(output);
        }

        let mut depth = 0i32;
        // Clone the maps for the recursive printer (it reads them; the member maps
        // deliberately persist across windows — see the module-level bug note).
        let nodes = self.nodes.clone();
        let deps = self.deps.clone();
        self.proc_node(&mut depth, &nodes, &deps, 0, output);

        let _ = write!(output, "  </SENTENCE>\n");
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.proc-node-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.proc-node-fn]
    /// C++ `void MatxinApplicator::procNode(int& depth, std::map<int, Node>& nodes,
    /// std::map<int, std::vector<int>>& deps, int n, std::ostream& output)`.
    /// Recursive depth-first printer of the dependency tree.
    pub fn proc_node<W: Write>(
        &self,
        depth: &mut i32,
        nodes: &BTreeMap<i32, Node>,
        deps: &BTreeMap<i32, Vec<i32>>,
        n: i32,
        output: &mut W,
    ) {
        // node = nodes[n]; v = deps[n]; (operator[] default-constructs on miss).
        let default_node = Node::default();
        let node = nodes.get(&n).unwrap_or(&default_node);
        let empty_v: Vec<i32> = Vec::new();
        let v = deps.get(&n).unwrap_or(&empty_v);
        *depth += 1;

        // si = node.si.data() + !node.si.empty() — skip the leading '@'.
        let si: String = if node.si.is_empty() {
            String::new()
        } else {
            node.si.chars().skip(1).collect()
        };

        if n != 0 {
            for _ in 0..(*depth * 2) {
                let _ = write!(output, " ");
            }
            if !v.is_empty() {
                let _ = write!(
                    output,
                    "<NODE ord=\"{}\" alloc=\"0\" form=\"{}\" lem=\"{}\" mi=\"{}\" si=\"{}\">\n",
                    node.self_, node.form, node.lemma, node.mi, si
                );
            } else {
                let _ = write!(
                    output,
                    "<NODE ord=\"{}\" alloc=\"0\" form=\"{}\" lem=\"{}\" mi=\"{}\" si=\"{}\"/>\n",
                    node.self_, node.form, node.lemma, node.mi, si
                );
                *depth -= 1;
            }
        }

        // found = any deps entry with first == n and a non-empty vector.
        let found = deps.iter().any(|(&k, val)| k == n && !val.is_empty());
        if !found {
            return;
        }
        for &it in v.iter() {
            self.proc_node(depth, nodes, deps, it, output);
        }

        if n != 0 {
            for _ in 0..(*depth * 2) {
                let _ = write!(output, " ");
            }
            let _ = write!(output, "</NODE>\n");
        }
        *depth -= 1;
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-wrapper-null-flush-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-wrapper-null-flush-fn]
    /// C++ `void MatxinApplicator::runGrammarOnTextWrapperNullFlush(...)`. Drives
    /// repeated grammar runs for null-flush mode.
    pub fn run_grammar_on_text_wrapper_null_flush<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        self.set_null_flush(false);
        self.running_with_null_flush = true;
        while !stream_eof(input) {
            self.run_grammar_on_text_impl(input, output);
            u_fputc('\0', output);
            u_fflush(output);
        }
        self.running_with_null_flush = false;
    }

    // [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn]
    /// C++ `void MatxinApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`.
    // Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
    // the ported reads were elided (see the deferred-I/O / driver notes).
    pub fn run_grammar_on_text<R, W>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        crate::error::catch_fatal(|| self.run_grammar_on_text_impl(input, output))
    }

    #[allow(unused_assignments, unused_variables)]
    fn run_grammar_on_text_impl<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        // ux_stdin/ux_stdout are Option<()> placeholders.
        if self.get_null_flush() {
            self.run_grammar_on_text_wrapper_null_flush(input, output);
            return;
        }

        // No-delimiter warnings.
        let no_hard = self.base.grammar.delimiters.is_none();
        let no_soft = self.base.grammar.soft_delimiters.is_none();
        if no_hard {
            if no_soft {
                tracing::warn!(
                    "Warning: No soft or hard delimiters defined in grammar. Hard limit of {} cohorts may break windows in unintended places.",
                    self.base.hard_limit
                );
            } else {
                tracing::warn!(
                    "Warning: No hard delimiters defined in grammar. Soft limit of {} cohorts may break windows in unintended places.",
                    self.base.soft_limit
                );
            }
        }

        let mut inchar: char = '\0';
        let mut superblank = false;
        let mut incohort = false;
        let mut firstblank: UString = String::new();

        self.base.index();

        let reset_after: u32 = (self.base.num_windows + 4) * 2 + 1;

        self.base.begintag = {
            let t = self
                .base
                .add_tag(STR_BEGINTAG, crate::tag::TagType::empty());
            self.base.grammar.single_tags_list.get(t.0).hash
        };
        self.base.endtag = {
            let t = self.base.add_tag(STR_ENDTAG, crate::tag::TagType::empty());
            self.base.grammar.single_tags_list.get(t.0).hash
        };

        let mut c_swindow: Option<SwId> = None;
        let mut c_cohort: Option<CohortId> = None;
        let mut c_reading: Option<ReadingId> = None;
        let mut l_swindow: Option<SwId> = None;

        self.base.gWindow.window_span = self.base.num_windows;

        ux_strip_bom(input);

        loop {
            // C++ `while ((inchar = u_fgetc(input)) != 0)` then `if (input.eof())
            // break;`. A read of '\0' terminates the loop (the `!= 0` guard); an
            // EOF (U_EOF) also terminates it (the `input.eof()` break).
            inchar = u_fgetc(input);
            if inchar == '\0' || inchar == U_EOF {
                break;
            }

            if inchar == '[' {
                superblank = true;
            }
            if inchar == ']' {
                superblank = false;
            }

            if inchar == '\\' && !incohort && !superblank {
                let n = u_fgetc(input);
                if let Some(cc) = c_cohort {
                    self.base.store.cohorts.get_mut(cc.0).text.push(inchar);
                    self.base.store.cohorts.get_mut(cc.0).text.push(n);
                } else if let Some(ls) = l_swindow {
                    self.base
                        .store
                        .single_windows
                        .get_mut(ls.0)
                        .text
                        .push(inchar);
                    self.base.store.single_windows.get_mut(ls.0).text.push(n);
                } else {
                    let _ = write!(output, "{inchar}");
                    let _ = write!(output, "{n}");
                }
                continue;
            }

            if inchar == '^' {
                incohort = true;
            }

            if superblank || inchar == ']' || !incohort {
                if let Some(cc) = c_cohort {
                    self.base.store.cohorts.get_mut(cc.0).text.push(inchar);
                } else if let Some(ls) = l_swindow {
                    self.base
                        .store
                        .single_windows
                        .get_mut(ls.0)
                        .text
                        .push(inchar);
                } else {
                    firstblank.push(inchar);
                }
                continue;
            }

            // We are at the start of a cohort.
            // Magic reading for the previous cohort.
            if let Some(cc) = c_cohort {
                if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                    self.base.init_empty_cohort(cc);
                }
            }
            // Soft-limit break.
            if let (Some(cc), Some(cs)) = (c_cohort, c_swindow) {
                let cohorts_size = self.base.store.single_windows.get(cs.0).cohorts.len() as u32;
                if cohorts_size >= self.base.soft_limit
                    && self.base.grammar.soft_delimiters.is_some()
                {
                    let sd = self.base.grammar.sets_list
                        [self.base.grammar.soft_delimiters.unwrap().0]
                        .number.get();
                    if self.base.does_set_match_cohort_normal(cc, sd, None) {
                        self.add_endtag_all(cc);
                        append_cohort(&mut self.base.gWindow, &mut self.base.store, cs, cc);
                        l_swindow = Some(cs);
                        c_swindow = None;
                        c_cohort = None;
                        self.base.numCohorts = self.base.numCohorts.wrapping_add(1);
                    }
                }
            }
            // Hard-limit break.
            if let (Some(cc), Some(cs)) = (c_cohort, c_swindow) {
                let cohorts_size = self.base.store.single_windows.get(cs.0).cohorts.len() as u32;
                let hard = cohorts_size >= self.base.hard_limit;
                let delim_match = self.base.grammar.delimiters.is_some() && {
                    let d =
                        self.base.grammar.sets_list[self.base.grammar.delimiters.unwrap().0].number.get();
                    self.base.does_set_match_cohort_normal(cc, d, None)
                };
                if hard || delim_match {
                    if !self.base.is_conv && cohorts_size >= self.base.hard_limit {
                        let wf_tid = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                        let wftag = self.base.grammar.single_tags_list.get(wf_tid.0).tag.clone();
                        tracing::warn!(
                            "Warning: Hard limit of {} cohorts reached at cohort {} (#{}) on line {} - forcing break.",
                            self.base.hard_limit,
                            wftag,
                            self.base.numCohorts,
                            self.base.numLines
                        );
                    }
                    self.add_endtag_all(cc);
                    append_cohort(&mut self.base.gWindow, &mut self.base.store, cs, cc);
                    l_swindow = Some(cs);
                    c_swindow = None;
                    c_cohort = None;
                    self.base.numCohorts = self.base.numCohorts.wrapping_add(1);
                }
            }
            // Create a window if none.
            if c_swindow.is_none() {
                let cs = self
                    .base
                    .gWindow
                    .alloc_append_single_window(&mut self.base.store);
                c_swindow = Some(cs);
                // 0th BOS cohort.
                let cc = alloc_cohort(&mut self.base.store, Some(cs));
                let gn = self.base.gWindow.cohort_counter;
                self.base.gWindow.cohort_counter = self.base.gWindow.cohort_counter.wrapping_add(1);
                self.base.store.cohorts.get_mut(cc.0).global_number = gn;
                self.base.store.cohorts.get_mut(cc.0).wordform = self.base.tag_begin;
                let cr = alloc_reading(&mut self.base.store, Some(cc));
                self.base.store.readings.get_mut(cr.0).baseform = Some(self.base.begintag);
                insert_if_exists(
                    &mut self.base.store.cohorts.get_mut(cc.0).possible_sets,
                    self.base.grammar.sets_any.as_ref(),
                );
                let bt = tag_by_hash(&self.base.grammar, self.base.begintag);
                self.base.add_tag_to_reading(cr, bt);
                append_reading(&mut self.base.store, cc, cr);
                append_cohort(&mut self.base.gWindow, &mut self.base.store, cs, cc);
                l_swindow = Some(cs);
                self.base.store.single_windows.get_mut(cs.0).text = firstblank.clone();
                firstblank.clear();
                c_cohort = None;
                self.base.numWindows = self.base.numWindows.wrapping_add(1);
            }
            let cs = c_swindow.unwrap();

            // Append the PREVIOUS cohort.
            if let Some(cc) = c_cohort {
                append_cohort(&mut self.base.gWindow, &mut self.base.store, cs, cc);
            }
            if self.base.gWindow.next.len() as u32 > self.base.num_windows {
                self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                self.base.run_grammar_on_window(output);
                if reset_after != 0 && self.base.numWindows % reset_after == 0 {
                    self.base.reset_indexes();
                }
            }

            // Allocate the new cohort.
            let cc = alloc_cohort(&mut self.base.store, Some(cs));
            c_cohort = Some(cc);
            let gn = self.base.gWindow.cohort_counter;
            self.base.gWindow.cohort_counter = self.base.gWindow.cohort_counter.wrapping_add(1);
            self.base.store.cohorts.get_mut(cc.0).global_number = gn;

            // Read the wordform.
            let mut wordform: UString = String::from("\"<");
            loop {
                inchar = u_fgetc(input);
                if inchar == '/' || inchar == '<' {
                    break;
                } else if inchar == '\\' {
                    inchar = u_fgetc(input);
                    wordform.push(inchar);
                } else {
                    wordform.push(inchar);
                }
            }
            wordform.push_str(">\"");
            let wf_tid = self.base.add_tag(&wordform, crate::tag::TagType::empty());
            self.base.store.cohorts.get_mut(cc.0).wordform = Some(wf_tid);
            self.base.numCohorts = self.base.numCohorts.wrapping_add(1);

            let mut current_reading: Vec<char> = Vec::new();
            c_reading = None;

            // Static reading.
            if inchar == '<' {
                let wread = alloc_reading(&mut self.base.store, Some(cc));
                self.base.store.cohorts.get_mut(cc.0).wread = Some(wread);
                let mut tagbuf: UString = String::new();
                loop {
                    inchar = u_fgetc(input);
                    if inchar == '\\' {
                        inchar = u_fgetc(input);
                        tagbuf.push(inchar);
                        continue;
                    }
                    if inchar == '<' {
                        continue;
                    }
                    if inchar == '>' {
                        let t = self.base.add_tag(&tagbuf, crate::tag::TagType::empty());
                        self.base.add_tag_to_reading(wread, t);
                        tagbuf.clear();
                        continue;
                    }
                    if inchar == '/' || inchar == '$' {
                        break;
                    }
                    tagbuf.push(inchar);
                    if inchar == '/' || inchar == '$' {
                        break;
                    }
                }
            }

            // Read the readings.
            while incohort {
                inchar = u_fgetc(input);
                if inchar == '\\' {
                    inchar = u_fgetc(input);
                    current_reading.push(inchar);
                    continue;
                }
                if inchar == '$' {
                    let cr = alloc_reading(&mut self.base.store, Some(cc));
                    c_reading = Some(cr);
                    if let Some(parent) = self.base.store.readings.get(cr.0).parent {
                        insert_if_exists(
                            &mut self.base.store.cohorts.get_mut(parent.0).possible_sets,
                            self.base.grammar.sets_any.as_ref(),
                        );
                    }
                    let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                    self.base.add_tag_to_reading(cr, wf);
                    self.process_reading(cr, &current_reading);
                    let mut cr = cr;
                    if self.base.grammar.sub_readings_ltr
                        && self.base.store.readings.get(cr.0).next.is_some()
                    {
                        cr = reverse_reading(&mut self.base.store, cr);
                    }
                    append_reading(&mut self.base.store, cc, cr);
                    c_reading = Some(cr);
                    self.base.numReadings = self.base.numReadings.wrapping_add(1);
                    current_reading.clear();
                    incohort = false;
                }
                if inchar == '/' {
                    let cr = alloc_reading(&mut self.base.store, Some(cc));
                    let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                    self.base.add_tag_to_reading(cr, wf);
                    self.process_reading(cr, &current_reading);
                    let mut cr2 = cr;
                    if self.base.grammar.sub_readings_ltr
                        && self.base.store.readings.get(cr2.0).next.is_some()
                    {
                        cr2 = reverse_reading(&mut self.base.store, cr2);
                    }
                    append_reading(&mut self.base.store, cc, cr2);
                    self.base.numReadings = self.base.numReadings.wrapping_add(1);
                    current_reading.clear();
                    continue;
                }
                current_reading.push(inchar);
            }

            // if (!cReading->baseform) warn
            let no_baseform = match c_reading {
                Some(cr) => self.base.store.readings.get(cr.0).baseform.is_none(),
                None => true,
            };
            if no_baseform {
                tracing::warn!(
                    "Warning: Line {} had no valid baseform.",
                    self.base.numLines
                );
            }
            self.base.numLines = self.base.numLines.wrapping_add(1);
        }

        if !firstblank.is_empty() {
            let _ = write!(output, "{firstblank}");
            firstblank.clear();
        }

        if let (Some(cc), Some(cs)) = (c_cohort, c_swindow) {
            append_cohort(&mut self.base.gWindow, &mut self.base.store, cs, cc);
            if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                self.base.init_empty_cohort(cc);
            }
            self.add_endtag_all(cc);
            c_reading = None;
            c_cohort = None;
            c_swindow = None;
        }
        let _ = (c_reading, c_cohort, c_swindow);

        // Run the grammar & print results.
        let _ = write!(output, "<corpus>\n");
        while !self.base.gWindow.next.is_empty() {
            self.base.gWindow.shuffle_windows_down(&mut self.base.store);
            self.base.run_grammar_on_window(output);
        }
        self.base.gWindow.shuffle_windows_down(&mut self.base.store);
        while !self.base.gWindow.previous.is_empty() {
            let tmp = self.base.gWindow.previous[0];
            self.print_single_window(tmp, output, false);
            let opt = Some(tmp);
            crate::single_window::free_swindow(&mut self.base.gWindow, &mut self.base.store, opt);
            self.base.gWindow.previous.remove(0);
        }

        if inchar != '\0' && inchar != '\u{FFFF}' {
            let _ = write!(output, "{inchar}");
        }
        let _ = write!(output, "</corpus>\n");
        u_fflush(output);
    }

    /// C++ `for (auto iter : cCohort->readings) addTagToReading(*iter, endtag);`.
    fn add_endtag_all(&mut self, cohort: CohortId) {
        let readings = self.base.store.cohorts.get(cohort.0).readings.clone();
        for r in readings {
            let et = tag_by_hash(&self.base.grammar, self.base.endtag);
            self.base.add_tag_to_reading(r, et);
        }
    }
}

// ---------------------------------------------------------------------------
// Port-infra: reading value-copy + comparator + char-stream reader.
// ---------------------------------------------------------------------------

// Wave 4 (w4-file-split-fmt): the verbatim Reading field-copy is
// consolidated in `crate::reading::clone_verbatim`.
use crate::reading::clone_verbatim as clone_reading_value;

/// `Reading::cmp_number` as an `Ordering` over two arena ids.
fn cmp_reading(store: &RuntimeStore, a: ReadingId, b: ReadingId) -> std::cmp::Ordering {
    let ra = store.readings.get(a.0);
    let rb = store.readings.get(b.0);
    if Reading::cmp_number(ra, rb) {
        std::cmp::Ordering::Less
    } else if Reading::cmp_number(rb, ra) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// `input.eof()` analog for the null-flush loop: peek one byte via Seek.
fn stream_eof<R: std::io::Read + std::io::Seek>(input: &mut R) -> bool {
    let mut b = [0u8; 1];
    match input.read(&mut b) {
        Ok(0) => true,
        Ok(_) => {
            let _ = input.seek(std::io::SeekFrom::Current(-1));
            false
        }
        Err(_) => true,
    }
}
