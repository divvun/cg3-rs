//! Wave-3 engine-phase `/test` facets for the CORE DATA MODEL symbols:
//! Cohort, CohortIterator (+ subclasses), Reading, Set, Rule, Tag,
//! ContextualTest, SingleWindow, Window.
//!
//! Strategy: DIRECT library API tests against the arena/store model (a
//! `RuntimeStore` owns the dynamic cohort/reading/single-window arenas; a
//! `Grammar` owns the static tag/set/rule/context arenas; the `Window` is the
//! engine-layer singleton threaded explicitly). One id
//! (`ContextualTest::markUsed`, ported as the PRIVATE `Grammar::context_mark_used`)
//! is driven through a fixture run of the `vislcg3` binary instead, because the
//! Rust port exposes no public entry point short of a full grammar load.

use std::path::{Path, PathBuf};
use std::process::Command;

use cg3::arena::{Arena, CohortId, CtxId, SwId};
use cg3::cohort::{
    CT_ENCLOSED, CT_NUM_CURRENT, CT_RELATED, CT_REMOVED, Cohort, DEP_NO_PARENT, alloc_cohort,
    allocate_append_reading, append_reading, append_reading_to, cohort_clear, cohort_dtor, detach,
    free_cohort, get_max, get_min, set_related, unignore_all, update_min_max,
};
use cg3::cohort_iterator::{
    ChildrenIterator, CohortIterator, CohortSetIter, DepAncestorIter, DepDescendentIter,
    DepParentIter, MultiCohortIterator, TopologyLeftIter, TopologyRightIter,
};
use cg3::contextual_test::{ContextualTest, POS_RIGHTMOST, POS_SELF, POS_SPAN_BOTH, copy_cntx};
use cg3::grammar::Grammar;
use cg3::inlines::{NUMERIC_MAX, NUMERIC_MIN, hash_value, hash_value_ustring};
use cg3::reading::{
    Reading, ReadingList, alloc_reading, alloc_reading_copy, free_reading, reading_clear,
    reading_copy, reading_rehash,
};
use cg3::rule::{
    FLAGS_EXCLS, RF_AFTER, RF_ALLOWLOOP, RF_BEFORE, RF_NEAREST, Rule, init_flag_excls,
};
use cg3::set::{ST_MAPPING, ST_SPECIAL, ST_USED, trie_reindex};
use cg3::single_window::{
    alloc_swindow, append_cohort, compare_Cohort, free_swindow, less_cohort, single_window_clear,
    single_window_destroy,
};
use cg3::store::RuntimeStore;
use cg3::strings::KEYWORDS;
use cg3::tag::{
    C_OPS, T_BASEFORM, T_CASE_INSENSITIVE, T_DEPENDENCY, T_MAPPING, T_NUMERICAL, T_REGEXP,
    T_RELATION, T_SPECIAL, T_TEXTUAL, T_USED, T_WORDFORM, Tag, TagVector, compare_tag,
    compare_tag_vector, equal_tag, fill_tagvector, parse_tag_raw,
};
use cg3::window::Window;

// ---------------------------------------------------------------------------
// Shared setup: a Window + RuntimeStore + one SingleWindow with `n` cohorts
// (global numbers 1..=n) appended via the real append_cohort wiring.
// ---------------------------------------------------------------------------
fn setup_window(n: u32) -> (RuntimeStore, Window, SwId, Vec<CohortId>) {
    let mut store = RuntimeStore::new();
    let mut w = Window::new(Some(0));
    let sw = w.alloc_append_single_window(&mut store);
    let mut ids = Vec::new();
    for g in 1..=n {
        let c = alloc_cohort(&mut store, Some(sw));
        store.cohorts.get_mut(c.0).global_number = g;
        append_cohort(&mut w, &mut store, sw, c);
        ids.push(c);
    }
    (store, w, sw, ids)
}

// ===========================================================================
// Cohort.cpp / Cohort.hpp
// ===========================================================================

// alloc_cohort (parent wired, dep_parent = DEP_NO_PARENT default), detach
// (sibling chain relink), cohort_clear (field reset + reading free + window-map
// erase + the `ignored`-not-cleared quirk), cohort_dtor (map erase + detach
// WITHOUT field reset), free_cohort (handle nulled + LIFO slot reuse).
// [spec:cg3:sem:cohort.cg3.alloc-cohort-fn/test]
// [spec:cg3:sem:cohort.cg3.free-cohort-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.cohort-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.clear-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.detach-fn/test]
#[test]
fn cohort_alloc_detach_clear_dtor_free() {
    let (mut store, mut w, sw, ids) = setup_window(3);
    let (c1, c2, c3) = (ids[0], ids[1], ids[2]);

    // alloc_cohort: parent set, everything else at C++ defaults.
    let fresh = alloc_cohort(&mut store, Some(sw));
    assert_eq!(store.cohorts.get(fresh.0).parent, Some(sw));
    assert_eq!(store.cohorts.get(fresh.0).dep_parent, DEP_NO_PARENT);
    assert!(store.cohorts.get(fresh.0).readings.is_empty());
    store.cohorts.free_slot(fresh.0);

    // detach: unlink c2, siblings re-wired around it.
    assert_eq!(store.cohorts.get(c1.0).next, Some(c2));
    detach(&mut store, c2);
    assert_eq!(store.cohorts.get(c1.0).next, Some(c3));
    assert_eq!(store.cohorts.get(c3.0).prev, Some(c1));
    assert_eq!(store.cohorts.get(c2.0).prev, None);
    assert_eq!(store.cohorts.get(c2.0).next, None);

    // clear: readings freed + fields reset + window maps erased; QUIRK: the
    // `ignored` list keeps its (now-dangling) ids.
    let r = allocate_append_reading(&mut store, c3);
    let ig = alloc_reading(&mut store, Some(c3));
    store.cohorts.get_mut(c3.0).ignored.push(ig);
    store.cohorts.get_mut(c3.0).dep_parent = 1;
    assert!(w.cohort_map.contains_key(&3));
    cohort_clear(&mut store, Some(&mut w), c3);
    assert!(!w.cohort_map.contains_key(&3), "clear erases from cohort_map");
    assert!(!w.dep_window.contains_key(&3));
    let c3r = store.cohorts.get(c3.0);
    assert_eq!(c3r.global_number, 0);
    assert_eq!(c3r.dep_parent, DEP_NO_PARENT);
    assert_eq!(c3r.parent, None);
    assert!(c3r.readings.is_empty());
    assert_eq!(c3r.ignored.len(), 1, "quirk: ignored NOT cleared");
    assert!(store.readings.try_get(r.0).is_none(), "readings freed");
    assert!(store.readings.try_get(ig.0).is_none(), "ignored readings freed");

    // dtor: erases from the window maps and detaches, but does NOT reset fields.
    assert!(w.cohort_map.contains_key(&1));
    cohort_dtor(&mut store, Some(&mut w), c1);
    assert!(!w.cohort_map.contains_key(&1));
    assert!(!w.dep_window.contains_key(&1));
    assert_eq!(store.cohorts.get(c1.0).global_number, 1, "dtor keeps fields");

    // free_cohort: nulls the handle; the slot is recycled LIFO by the next alloc.
    let mut h = Some(c2);
    free_cohort(&mut store, Some(&mut w), &mut h);
    assert_eq!(h, None, "handle nulled like the C++ Cohort*&");
    let reused = alloc_cohort(&mut store, Some(sw));
    assert_eq!(reused, c2, "freed slot reused (pool semantics)");
}

// appendReading (member-list + external-list overloads: number staged from the
// post-push size when still 0; CT_NUM_CURRENT cleared) and
// allocateAppendReading (fresh reading parented to the cohort, staged number).
// [spec:cg3:sem:cohort.cg3.cohort.append-reading-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.allocate-append-reading-fn/test]
#[test]
fn cohort_append_readings() {
    let (mut store, _w, _sw, ids) = setup_window(1);
    let c = ids[0];

    store.cohorts.get_mut(c.0).r#type |= CT_NUM_CURRENT;
    let r1 = alloc_reading(&mut store, None); // parent None -> number 0
    assert_eq!(store.readings.get(r1.0).number, 0);
    append_reading(&mut store, c, r1);
    // post-push size 1 -> 1*1000+1000 = 2000
    assert_eq!(store.readings.get(r1.0).number, 2000);
    assert_eq!(
        store.cohorts.get(c.0).r#type & CT_NUM_CURRENT,
        0,
        "append clears CT_NUM_CURRENT"
    );

    let r2 = alloc_reading(&mut store, None);
    append_reading(&mut store, c, r2);
    assert_eq!(store.readings.get(r2.0).number, 3000);

    // 2-arg overload against an external list.
    let mut staging = ReadingList::new();
    let r3 = alloc_reading(&mut store, None);
    append_reading_to(&mut store, c, r3, &mut staging);
    assert_eq!(staging, vec![r3]);
    assert_eq!(store.readings.get(r3.0).number, 2000);

    // allocateAppendReading: alloc_reading(this) stages a non-zero number from
    // the PRE-push count, so the post-push if is dead — number stays staged.
    let r4 = allocate_append_reading(&mut store, c);
    assert_eq!(store.readings.get(r4.0).parent, Some(c));
    assert_eq!(store.readings.get(r4.0).number, 3000); // 2 pre-push readings
    assert_eq!(store.cohorts.get(c.0).readings.len(), 3);
}

// updateMinMax (per-comparison-hash strict min/max over `readings` only, cached
// behind CT_NUM_CURRENT), getMin/getMax (cache refresh + NUMERIC_MIN/MAX
// fallback for an absent key).
// [spec:cg3:sem:cohort.cg3.cohort.update-min-max-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.get-min-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.get-max-fn/test]
#[test]
fn cohort_numeric_min_max() {
    let mut g = Grammar::default();
    let t5 = g.allocate_tag("<n=5>");
    let t10 = g.allocate_tag("<n=10>");
    let key = g.single_tags_list[t5.0].comparison_hash;
    assert_ne!(key, 0);
    assert_eq!(key, g.single_tags_list[t10.0].comparison_hash);

    let (mut store, _w, _sw, ids) = setup_window(1);
    let c = ids[0];
    let r1 = allocate_append_reading(&mut store, c);
    let r2 = allocate_append_reading(&mut store, c);
    let h5 = g.single_tags_list[t5.0].hash;
    let h10 = g.single_tags_list[t10.0].hash;
    store.readings.get_mut(r1.0).tags_numerical.insert(h5, t5);
    store.readings.get_mut(r2.0).tags_numerical.insert(h10, t10);

    update_min_max(&mut store, &g, c);
    assert_ne!(store.cohorts.get(c.0).r#type & CT_NUM_CURRENT, 0);
    assert_eq!(get_min(&mut store, &g, c, key), 5.0);
    assert_eq!(get_max(&mut store, &g, c, key), 10.0);
    // Absent key -> sentinel extremes.
    assert_eq!(get_min(&mut store, &g, c, 0xDEAD), NUMERIC_MIN);
    assert_eq!(get_max(&mut store, &g, c, 0xDEAD), NUMERIC_MAX);

    // Cache: adding a smaller value is invisible until CT_NUM_CURRENT drops.
    let t1 = g.allocate_tag("<n=1>");
    let h1 = g.single_tags_list[t1.0].hash;
    store.readings.get_mut(r1.0).tags_numerical.insert(h1, t1);
    assert_eq!(get_min(&mut store, &g, c, key), 5.0, "stale cache honoured");
    store.cohorts.get_mut(c.0).r#type &= !CT_NUM_CURRENT;
    assert_eq!(get_min(&mut store, &g, c, key), 1.0, "recomputed after inval");
}

// addChild/remChild (dep_children sorted-set ops), addRelation (grew?),
// setRelation (single-target overwrite + relations_input erase), remRelation
// (shrank?), setRelated (CT_RELATED + noprint=false sweep), unignoreAll
// (ignored -> readings move, deleted cleared).
// [spec:cg3:sem:cohort.cg3.cohort.add-child-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.rem-child-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.add-relation-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.set-relation-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.rem-relation-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.set-related-fn/test]
// [spec:cg3:sem:cohort.cg3.cohort.unignore-all-fn/test]
#[test]
fn cohort_children_relations_related_unignore() {
    let mut c = Cohort::default();

    c.add_child(7);
    c.add_child(3);
    c.add_child(7); // dedup no-op
    assert_eq!(c.dep_children.size(), 2);
    c.rem_child(7);
    assert!(!c.dep_children.contains(7));
    c.rem_child(99); // absent -> no-op
    assert_eq!(c.dep_children.size(), 1);

    assert!(c.add_relation(11, 100), "new target grows the set");
    assert!(!c.add_relation(11, 100), "duplicate does not grow");
    assert!(c.add_relation(11, 101));

    // setRelation: {100,101} != {200} -> replaced, true.
    c.relations_input.insert(11, Default::default());
    assert!(c.set_relation(11, 200));
    assert!(!c.relations_input.contains_key(&11), "relations_input erased");
    assert_eq!(c.relations[&11].size(), 1);
    assert!(!c.set_relation(11, 200), "already exactly the target -> false");

    assert!(c.rem_relation(11, 200), "shrank -> true");
    assert!(!c.rem_relation(11, 200), "already gone -> false");
    assert!(!c.rem_relation(42, 1), "unknown relation -> false");

    // setRelated / unignoreAll need the store (they write Readings).
    let (mut store, _w, _sw, ids) = setup_window(1);
    let co = ids[0];
    let r = allocate_append_reading(&mut store, co);
    store.readings.get_mut(r.0).noprint = true;
    set_related(&mut store, co);
    assert_ne!(store.cohorts.get(co.0).r#type & CT_RELATED, 0);
    assert!(!store.readings.get(r.0).noprint);

    unignore_all(&mut store, co); // empty ignored -> no-op
    assert_eq!(store.cohorts.get(co.0).readings.len(), 1);
    let ig = alloc_reading(&mut store, Some(co));
    store.readings.get_mut(ig.0).deleted = true;
    store.cohorts.get_mut(co.0).ignored.push(ig);
    unignore_all(&mut store, co);
    let coh = store.cohorts.get(co.0);
    assert!(coh.ignored.is_empty());
    assert_eq!(coh.readings.last(), Some(&ig), "appended at the end");
    assert!(!store.readings.get(ig.0).deleted);
}

// ===========================================================================
// CohortIterator.cpp
// ===========================================================================

// Base CohortIterator: ctor/advance/current (the ctor id covers all three, as
// in the source annotations), operator== (m_cohort-only sentinel compare),
// reset (re-seat without allocation).
// [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.operator-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.reset-fn/test]
#[test]
fn cohort_iterator_base() {
    let c = CohortId(5);
    let mut it = CohortIterator::new(Some(c), None, true);
    assert_eq!(it.current(), Some(c));
    assert!(it.m_span);
    let end = CohortIterator::new(None, None, false);
    assert!(!it.equals(&end));
    it.advance(); // base operator++ nulls m_cohort
    assert_eq!(it.current(), None);
    assert!(it.equals(&end), "operator== compares only m_cohort");
    it.reset(Some(c), Some(CtxId(0)), false);
    assert_eq!(it.current(), Some(c));
    assert!(!it.m_span);
    assert_eq!(it.m_test, Some(CtxId(0)));
}

// TopologyLeftIter / TopologyRightIter: sibling-chain walk skipping
// CT_ENCLOSED cohorts, stopping at a window boundary unless the test allows
// spanning (POS_SPAN_*/m_span).
// [spec:cg3:sem:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn/test]
#[test]
fn topology_iterators() {
    let mut store = RuntimeStore::new();
    let mut w = Window::new(Some(0));
    let sw1 = w.alloc_append_single_window(&mut store);
    let sw2 = w.alloc_append_single_window(&mut store); // linked sw1 <-> sw2
    let mut mk = |sw: SwId, gn: u32| {
        let c = alloc_cohort(&mut store, Some(sw));
        store.cohorts.get_mut(c.0).global_number = gn;
        append_cohort(&mut w, &mut store, sw, c);
        c
    };
    let c1 = mk(sw1, 1);
    let c2 = mk(sw1, 2);
    let c3 = mk(sw1, 3);
    let c4 = mk(sw2, 4);
    store.cohorts.get_mut(c2.0).r#type |= CT_ENCLOSED;

    let mut g = Grammar::default();
    let ctx0 = g.allocate_contextual_test(); // pos = 0
    let ctx_span = g.allocate_contextual_test();
    g.contexts_arena[ctx_span.0].pos = POS_SPAN_BOTH;

    // Left from c3: skips enclosed c2, lands on c1; then walks off the front.
    let mut li = TopologyLeftIter::new(Some(c3), Some(ctx0), false);
    li.advance(&store, &g);
    assert_eq!(li.base.current(), Some(c1), "enclosed cohort skipped");
    li.advance(&store, &g);
    assert_eq!(li.base.current(), None);

    // Right from c3 without span: c4 is in the next window -> boundary -> end.
    let mut ri = TopologyRightIter::new(Some(c3), Some(ctx0), false);
    ri.advance(&store, &g);
    assert_eq!(ri.base.current(), None, "window boundary without span");
    // With POS_SPAN_BOTH the boundary may be crossed.
    let mut ri = TopologyRightIter::new(Some(c3), Some(ctx_span), false);
    ri.advance(&store, &g);
    assert_eq!(ri.base.current(), Some(c4), "spanning test crosses windows");
}

// DepParentIter: ctor immediately advances onto the first dependency parent;
// operator++ climbs via cohort_map with the m_seen cycle guard and the
// CT_REMOVED bail-out; reset clears m_seen and re-advances.
// [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.reset-fn/test]
#[test]
fn dep_parent_iterator() {
    let (mut store, w, _sw, ids) = setup_window(3);
    let (c1, c2, c3) = (ids[0], ids[1], ids[2]);
    store.cohorts.get_mut(c2.0).dep_parent = 1;
    store.cohorts.get_mut(c3.0).dep_parent = 2;
    let mut g = Grammar::default();
    let ctx = g.allocate_contextual_test();

    let mut it = DepParentIter::new(Some(c3), Some(ctx), false, &store, &g, &w);
    assert_eq!(it.base.current(), Some(c2), "ctor pre-advances onto the parent");
    it.advance(&store, &g, &w);
    assert_eq!(it.base.current(), Some(c1));
    it.advance(&store, &g, &w); // c1 has DEP_NO_PARENT
    assert_eq!(it.base.current(), None);

    it.reset(Some(c3), Some(ctx), false, &store, &g, &w);
    assert_eq!(it.base.current(), Some(c2), "reset clears m_seen and re-seats");

    // Cycle guard: c1 -> c3 closes a loop; the duplicate m_seen hit ends it.
    store.cohorts.get_mut(c1.0).dep_parent = 3;
    it.reset(Some(c3), Some(ctx), false, &store, &g, &w);
    assert_eq!(it.base.current(), Some(c2));
    it.advance(&store, &g, &w);
    assert_eq!(it.base.current(), Some(c1));
    it.advance(&store, &g, &w);
    assert_eq!(it.base.current(), None, "cycle terminated by m_seen");

    // CT_REMOVED parent kills the walk outright.
    store.cohorts.get_mut(c2.0).r#type |= CT_REMOVED;
    let it = DepParentIter::new(Some(c3), Some(ctx), false, &store, &g, &w);
    assert_eq!(it.base.current(), None);
}

// DepDescendentIter (BFS transitive closure of dep_children, sorted by
// less_Cohort; POS_SELF adds the origin) and DepAncestorIter (dep_parent climb;
// POS_RIGHTMOST reverses) — ctor(+advance) and reset for each.
// [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.reset-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.reset-fn/test]
#[test]
fn dep_descendent_and_ancestor_iterators() {
    let (mut store, w, _sw, ids) = setup_window(4);
    let (c1, c2, c3, c4) = (ids[0], ids[1], ids[2], ids[3]);
    // Tree: c1 -> {c2, c3}; c2 -> {c4}.
    store.cohorts.get_mut(c1.0).dep_children.insert(2);
    store.cohorts.get_mut(c1.0).dep_children.insert(3);
    store.cohorts.get_mut(c2.0).dep_children.insert(4);
    store.cohorts.get_mut(c2.0).dep_parent = 1;
    store.cohorts.get_mut(c3.0).dep_parent = 1;
    store.cohorts.get_mut(c4.0).dep_parent = 2;

    let mut g = Grammar::default();
    let ctx = g.allocate_contextual_test();
    let ctx_self = g.allocate_contextual_test();
    g.contexts_arena[ctx_self.0].pos = POS_SELF;
    let ctx_rr = g.allocate_contextual_test();
    g.contexts_arena[ctx_rr.0].pos = POS_RIGHTMOST;

    let mut di = DepDescendentIter::new(Some(c1), Some(ctx), false, &store, &g, &w);
    assert_eq!(di.base.current(), Some(c2), "direct + transitive, sorted");
    di.advance();
    assert_eq!(di.base.current(), Some(c3));
    di.advance();
    assert_eq!(di.base.current(), Some(c4), "grandchild found by the BFS");
    di.advance();
    assert_eq!(di.base.current(), None);
    // reset with POS_SELF: the origin joins the set (c1 sorts first).
    di.reset(Some(c1), Some(ctx_self), false, &store, &g, &w);
    assert_eq!(di.base.current(), Some(c1));

    let mut ai = DepAncestorIter::new(Some(c4), Some(ctx), false, &store, &g, &w);
    assert_eq!(ai.base.current(), Some(c1), "ancestors sorted by local_number");
    ai.advance();
    assert_eq!(ai.base.current(), Some(c2));
    ai.advance();
    assert_eq!(ai.base.current(), None);
    // reset with POS_RIGHTMOST reverses the chain.
    ai.reset(Some(c4), Some(ctx_rr), false, &store, &g, &w);
    assert_eq!(ai.base.current(), Some(c2));
}

// CohortSetIter (ctor + span-filtered advance incl. the faithful re-yield bug,
// addCohort's sorted insert + cursor rewind), MultiCohortIterator
// (ctor/advance/current + operator==), ChildrenIterator (ctor + advance
// installing the inner CohortSetIter and bumping m_depth).
// [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.add-cohort-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.operator-fn/test]
// [spec:cg3:sem:cohort-iterator.cg3.children-iterator.children-iterator-fn/test]
#[test]
fn cohort_set_multi_and_children_iterators() {
    let (mut store, _w, _sw, ids) = setup_window(2);
    let (c1, c2) = (ids[0], ids[1]);
    let mut g = Grammar::default();
    let ctx = g.allocate_contextual_test();

    let mut csi = CohortSetIter::new(Some(c1), Some(ctx), false);
    assert_eq!(csi.m_origcohort, Some(c1));
    csi.add_cohort(&store, c2);
    csi.add_cohort(&store, c1); // sorted before c2, cursor rewound to begin()
    assert_eq!(csi.m_cohortset, vec![c1, c2]);
    assert_eq!(csi.m_cohortsetiter, 0);
    csi.advance(&store, &g);
    assert_eq!(csi.base.current(), Some(c1), "same window, pos 0 accepted");
    csi.advance(&store, &g);
    assert_eq!(
        csi.base.current(),
        Some(c1),
        "faithful re-yield bug: cursor not advanced on a match"
    );

    let mut mi = MultiCohortIterator::new(Some(c1), Some(ctx), false);
    assert!(mi.current().is_none(), "no inner iterator yet");
    let mend = MultiCohortIterator::new(None, None, false);
    assert!(!mi.equals(&mend));
    mi.advance();
    assert!(mi.equals(&mend), "operator== compares only m_cohort");

    store.cohorts.get_mut(c1.0).dep_children.insert(2);
    let mut ch = ChildrenIterator::new(Some(c1), Some(ctx), false);
    assert_eq!(ch.m_depth, 0);
    ch.advance(&store);
    assert_eq!(ch.m_depth, 1);
    assert!(
        ch.base.m_cohortiter.is_some(),
        "non-empty dep_children installs an inner CohortSetIter"
    );
}

// ===========================================================================
// Reading.cpp
// ===========================================================================

// alloc_reading(const Reading&) (copy semantics + pooled-vs-new
// immutable/active divergence + deep next-chain clone), free_reading (handle
// null-out + slot recycle), Reading copy ctor (reading_copy: number+100,
// matched_* cleared, deep clone), Reading::clear (full reset + next chain
// freed), Reading::allocateReading (thin alloc_reading(Cohort*) wrapper).
// [spec:cg3:sem:reading.cg3.alloc-reading-fn/test]
// [spec:cg3:sem:reading.cg3.free-reading-fn/test]
// [spec:cg3:sem:reading.cg3.reading.reading-fn/test]
// [spec:cg3:sem:reading.cg3.reading.clear-fn/test]
// [spec:cg3:sem:reading.cg3.reading.allocate-reading-fn/test]
#[test]
fn reading_alloc_copy_free_clear() {
    let (mut store, _w, _sw, ids) = setup_window(1);
    let c = ids[0];

    // allocateReading(Cohort*): number staged from the cohort's reading count.
    let r0 = Reading::allocate_reading(&mut store, Some(c));
    assert_eq!(store.readings.get(r0.0).parent, Some(c));
    assert_eq!(store.readings.get(r0.0).number, 1000);
    let rn = Reading::allocate_reading(&mut store, None);
    assert_eq!(store.readings.get(rn.0).number, 0, "null parent -> number 0");

    // Copy-alloc, fresh-slot branch: immutable/active KEPT, matched_* cleared,
    // number+100, next chain deep-cloned.
    let child = alloc_reading(&mut store, None);
    store.readings.get_mut(child.0).number = 7;
    let mut src = Reading::default();
    src.number = 500;
    src.immutable = true;
    src.active = true;
    src.matched_target = true;
    src.next = Some(child);
    let fresh = alloc_reading_copy(&mut store, &src);
    {
        let f = store.readings.get(fresh.0);
        assert_eq!(f.number, 600);
        assert!(f.immutable && f.active, "new-slot branch keeps the flags");
        assert!(!f.matched_target, "matched_* always cleared");
        let fc = f.next.expect("next chain deep-cloned");
        assert_ne!(fc, child, "clone, not an alias");
        assert_eq!(store.readings.get(fc.0).number, 107);
    }

    // Copy-alloc, pooled branch: a reused slot forces immutable/active false.
    let mut junk = Some(alloc_reading(&mut store, None));
    let junk_id = junk.unwrap();
    free_reading(&mut store, &mut junk);
    assert_eq!(junk, None, "free_reading nulls the handle");
    let mut src2 = Reading::default();
    src2.immutable = true;
    src2.active = true;
    let pooled = alloc_reading_copy(&mut store, &src2);
    assert_eq!(pooled, junk_id, "freed slot reused");
    let p = store.readings.get(pooled.0);
    assert!(!p.immutable && !p.active, "pooled branch forces the flags off");

    // Copy ctor by value (Reading::Reading(const Reading&)).
    let copied = reading_copy(&mut store, &src);
    assert_eq!(copied.number, 600);
    assert!(!copied.matched_target);
    assert_ne!(copied.next, Some(child), "next deep-cloned into a new slot");

    // clear: resets everything and frees the next chain.
    let tail = alloc_reading(&mut store, None);
    {
        let f = store.readings.get_mut(fresh.0);
        // splice a known chain end for the free assertion
        let old_child = f.next.unwrap();
        store.readings.get_mut(old_child.0).next = Some(tail);
    }
    reading_clear(&mut store, fresh);
    let f = store.readings.get(fresh.0);
    assert_eq!(f.number, 0);
    assert!(!f.immutable && !f.active);
    assert_eq!(f.next, None);
    assert!(store.readings.try_get(tail.0).is_none(), "chain freed recursively");
}

// Reading::rehash (tags fold skipping the mapping hash; plain vs mapped hash;
// recursive next-chain fold) and Reading::cmp_number (number asc, hash
// tie-break).
// [spec:cg3:sem:reading.cg3.reading.rehash-fn/test]
// [spec:cg3:sem:reading.cg3.reading.cmp-number-fn/test]
#[test]
fn reading_rehash_and_cmp_number() {
    let mut g = Grammar::default();
    let ta = g.allocate_tag("aa");
    let tb = g.allocate_tag("bb");
    let tm = g.allocate_tag("mapped");
    let (ha, hb, hm) = (
        g.single_tags_list[ta.0].hash,
        g.single_tags_list[tb.0].hash,
        g.single_tags_list[tm.0].hash,
    );

    let mut store = RuntimeStore::new();
    let r = alloc_reading(&mut store, None);
    {
        let rd = store.readings.get_mut(r.0);
        rd.tags.insert(ha);
        rd.tags.insert(hb);
        rd.tags.insert(hm);
        rd.mapping = Some(tm);
    }
    let got = reading_rehash(&mut store, &g, r);

    // Expected: fold sorted tag hashes skipping the mapping hash, then fold it.
    let mut sorted = vec![ha, hb, hm];
    sorted.sort_unstable();
    let mut exp = 0u32;
    for &h in &sorted {
        if h != hm {
            exp = hash_value(h, exp);
        }
    }
    let exp_plain = exp;
    exp = hash_value(hm, exp);
    assert_eq!(got, exp);
    assert_eq!(store.readings.get(r.0).hash, exp);
    assert_eq!(store.readings.get(r.0).hash_plain, exp_plain);
    assert_ne!(exp, exp_plain, "mapping folded into hash but not hash_plain");

    // Sub-reading chain: the child is rehashed and folded in.
    let sub = alloc_reading(&mut store, None);
    store.readings.get_mut(sub.0).tags.insert(ha);
    store.readings.get_mut(r.0).next = Some(sub);
    let got2 = reading_rehash(&mut store, &g, r);
    let sub_hash = store.readings.get(sub.0).hash;
    assert_eq!(sub_hash, hash_value(ha, 0));
    assert_eq!(got2, hash_value(sub_hash, exp));

    // cmp_number: strict-weak (number asc, hash tie-break).
    let mut a = Reading::default();
    let mut b = Reading::default();
    a.number = 1;
    b.number = 2;
    assert!(Reading::cmp_number(&a, &b));
    assert!(!Reading::cmp_number(&b, &a));
    b.number = 1;
    a.hash = 10;
    b.hash = 20;
    assert!(Reading::cmp_number(&a, &b), "hash tie-break");
    assert!(!Reading::cmp_number(&a, &a), "irreflexive");
}

// ===========================================================================
// Set.cpp / Set.hpp — driven through the grammar arena (Set::rehash/reindex/
// markUsed are grammar-associated fns in the port).
// ===========================================================================

// Set::empty (four containers only), setName (_G_<line>_<to>_ synthesis),
// rehash (content hash stored + differs by content), reindex (derived
// ST_SPECIAL/ST_MAPPING from the tries), markUsed (ST_USED + tag T_USED),
// trie_reindex (free helper flag accumulation), ~Set (drop glue via
// destroy_set's arena free running trie_delete).
// [spec:cg3:sem:set.cg3.set.empty-fn/test]
// [spec:cg3:sem:set.cg3.set.set-name-fn/test]
// [spec:cg3:sem:set.cg3.set.rehash-fn/test]
// [spec:cg3:sem:set.cg3.set.reindex-fn/test]
// [spec:cg3:sem:set.cg3.set.mark-used-fn/test]
// [spec:cg3:sem:set.cg3.set.set-fn/test]
// [spec:cg3:sem:set.cg3.trie-reindex-fn/test]
#[test]
fn set_name_hash_reindex_markused_drop() {
    let mut g = Grammar::default();
    let s = g.allocate_set();
    assert!(g.sets_list[s.0].empty(), "fresh set is empty");

    let tx = g.allocate_tag("x");
    g.add_tag_to_set(tx, s);
    assert!(!g.sets_list[s.0].empty(), "plain tag lands in the trie");

    // setName: explicit id and the rand() fallback for 0.
    g.sets_list.get_mut(s.0).line = 7;
    g.sets_list.get_mut(s.0).set_name(42, &mut g.rand_state);
    assert_eq!(g.sets_list[s.0].name, "_G_7_42_");
    g.sets_list.get_mut(s.0).set_name(0, &mut g.rand_state);
    let name = g.sets_list[s.0].name.clone();
    assert!(name.starts_with("_G_7_") && name.ends_with('_') && name != "_G_7_0_");

    // rehash: stored, non-zero, content-sensitive.
    let h1 = cg3::set::Set::rehash(&mut g, s);
    assert_ne!(h1, 0);
    assert_eq!(g.sets_list[s.0].hash, h1);
    let s2 = g.allocate_set();
    let ty = g.allocate_tag("y");
    g.add_tag_to_set(ty, s2);
    let h2 = cg3::set::Set::rehash(&mut g, s2);
    assert_ne!(h1, h2, "different tag content, different hash");

    // reindex: numeric tag (T_SPECIAL) in trie_special -> ST_SPECIAL; a
    // T_MAPPING tag in the plain trie -> ST_MAPPING (via trie_reindex).
    let tnum = g.allocate_tag("<n=5>");
    assert_ne!(g.single_tags_list[tnum.0].r#type & T_SPECIAL, 0);
    g.add_tag_to_set(tnum, s);
    g.single_tags_list.get_mut(tx.0).r#type |= T_MAPPING;
    cg3::set::Set::reindex(&mut g, s);
    let sty = g.sets_list[s.0].r#type;
    assert_ne!(sty & ST_SPECIAL, 0);
    assert_ne!(sty & ST_MAPPING, 0);

    // trie_reindex directly on each trie.
    assert_eq!(trie_reindex(&g.sets_list[s.0].trie, &g), ST_MAPPING as u8);
    assert_eq!(trie_reindex(&g.sets_list[s.0].trie_special, &g), ST_SPECIAL as u8);

    // markUsed: the set and every referenced tag.
    cg3::set::Set::mark_used(&mut g, s);
    assert_ne!(g.sets_list[s.0].r#type & ST_USED, 0);
    assert_ne!(g.single_tags_list[tx.0].r#type & T_USED, 0);
    assert_ne!(g.single_tags_list[tnum.0].r#type & T_USED, 0);

    // ~Set: destroy_set frees the arena slot, dropping the populated Set (the
    // Drop impl runs trie_delete on both tries).
    g.destroy_set(s);
    assert!(g.sets_list.try_get(s.0).is_none());
}

// ===========================================================================
// Rule.cpp / Rule.hpp
// ===========================================================================

// Rule() defaults (K_IGNORE type, zeroed/null members), setName (nullable
// UChar*), addContextualTest (push_front onto the passed head list),
// reverseContextualTests (tests + dep_tests), init_flag_excls / FLAGS_EXCLS
// (mutual-exclusion masks per flag bit).
// [spec:cg3:sem:rule.cg3.rule.rule-fn/test]
// [spec:cg3:sem:rule.cg3.rule.set-name-fn/test]
// [spec:cg3:sem:rule.cg3.rule.add-contextual-test-fn/test]
// [spec:cg3:sem:rule.cg3.rule.reverse-contextual-tests-fn/test]
// [spec:cg3:sem:rule.cg3.init-flag-excls-fn/test]
#[test]
fn rule_defaults_name_tests_flags() {
    let mut r = Rule::default();
    assert_eq!(r.r#type, KEYWORDS::K_IGNORE);
    assert_eq!(r.flags, 0);
    assert_eq!(r.section, 0);
    assert!(r.name.is_empty() && r.maplist.is_none() && r.dep_target.is_none());

    r.set_name(Some("myrule"));
    assert_eq!(r.name, "myrule");
    r.set_name(None); // nullptr -> cleared
    assert!(r.name.is_empty());

    // addContextualTest front-inserts; reverse flips both lists.
    Rule::add_contextual_test(CtxId(1), &mut r.tests);
    Rule::add_contextual_test(CtxId(2), &mut r.tests);
    assert_eq!(r.tests.iter().copied().collect::<Vec<_>>(), vec![CtxId(2), CtxId(1)]);
    Rule::add_contextual_test(CtxId(3), &mut r.dep_tests);
    Rule::add_contextual_test(CtxId(4), &mut r.dep_tests);
    r.reverse_contextual_tests();
    assert_eq!(r.tests.iter().copied().collect::<Vec<_>>(), vec![CtxId(1), CtxId(2)]);
    assert_eq!(r.dep_tests.iter().copied().collect::<Vec<_>>(), vec![CtxId(3), CtxId(4)]);

    // init_flag_excls: bit 0/1 (NEAREST/ALLOWLOOP) share a group; bit 27/28
    // (BEFORE/AFTER) share another; a groupless flag (SUB, bit 23) yields 0.
    assert_eq!(init_flag_excls(0), RF_NEAREST | RF_ALLOWLOOP);
    assert_eq!(init_flag_excls(1), RF_NEAREST | RF_ALLOWLOOP);
    assert_eq!(init_flag_excls(27), RF_BEFORE | RF_AFTER);
    assert_eq!(init_flag_excls(23), 0);
    assert_eq!(FLAGS_EXCLS[28], RF_BEFORE | RF_AFTER, "make_array expansion");
}

// ===========================================================================
// Tag.cpp / Tag.hpp
// ===========================================================================

// parseTagRaw (wordform/baseform classification, numeric <...> delegation,
// #x->y dependency, ID:n and R:name:n relation forms with interned relation
// tag) and parseNumeric (operator/value parsing incl. MAX and reject paths).
// [spec:cg3:sem:tag.cg3.tag.parse-tag-raw-fn/test]
// [spec:cg3:sem:tag.cg3.tag.parse-numeric-fn/test]
#[test]
fn tag_parse_raw_and_numeric() {
    let mut g = Grammar::default();

    let wf = g.allocate_tag("\"<word>\"");
    let wt = g.single_tags_list[wf.0].r#type;
    assert_ne!(wt & T_WORDFORM, 0);
    assert_ne!(wt & T_TEXTUAL, 0);
    let bf = g.allocate_tag("\"base\"");
    assert_ne!(g.single_tags_list[bf.0].r#type & T_BASEFORM, 0);

    // Dependency tag (both ASCII and the code path via parse_tag_raw directly).
    let dep = g.allocate_tag("#2->1");
    let d = &g.single_tags_list[dep.0];
    assert_ne!(d.r#type & T_DEPENDENCY, 0);
    assert_eq!((d.dep_self, d.dep_parent), (2, 1));

    let mut t = Tag::default();
    parse_tag_raw(&mut t, "ID:5", &mut g);
    assert_ne!(t.r#type & T_RELATION, 0);
    assert_eq!(t.dep_self, 5);

    // R:name:n interns the relation-name tag and caches its hash.
    let mut t = Tag::default();
    parse_tag_raw(&mut t, "R:mark:4", &mut g);
    assert_ne!(t.r#type & T_RELATION, 0);
    assert_eq!(t.dep_parent, 4);
    let mark = g.allocate_tag("mark"); // dedups to the tag interned above
    assert_eq!(t.comparison_hash, g.single_tags_list[mark.0].hash);

    // parseNumeric operators and values.
    let mut t = Tag::default();
    t.tag = "<w>=12>".to_string();
    t.parse_numeric(false);
    assert_eq!(t.comparison_op, C_OPS::OP_GREATEREQUALS);
    assert_eq!(t.comparison_val, 12.0);
    assert_ne!(t.r#type & T_NUMERICAL, 0);
    assert_eq!(t.comparison_hash, hash_value_ustring("w", 0));

    let mut t = Tag::default();
    t.tag = "<w<3>".to_string();
    t.parse_numeric(false);
    assert_eq!(t.comparison_op, C_OPS::OP_LESSTHAN);
    assert_eq!(t.comparison_val, 3.0);

    let mut t = Tag::default();
    t.tag = "<w=MAX>".to_string();
    t.parse_numeric(false);
    assert_eq!(t.comparison_val, NUMERIC_MAX);

    let mut t = Tag::default();
    t.tag = "<w=abc>".to_string();
    t.parse_numeric(false);
    assert_eq!(t.r#type & T_NUMERICAL, 0, "non-numeric value rejected");
    assert_eq!(t.comparison_op, C_OPS::OP_NOP);
}

// Tag copy ctor (Clone: tag_raw NOT copied — quirk; vs_names copied), rehash
// (plain hash + flag markers + seed fold), markUsed, allocateVsSets/VsNames
// (lazy, idempotent), toUString (prefix/suffix reconstruction + escaping +
// tag_raw passthrough).
// [spec:cg3:sem:tag.cg3.tag.tag-fn/test]
// [spec:cg3:sem:tag.cg3.tag.rehash-fn/test]
// [spec:cg3:sem:tag.cg3.tag.mark-used-fn/test]
// [spec:cg3:sem:tag.cg3.tag.allocate-vs-sets-fn/test]
// [spec:cg3:sem:tag.cg3.tag.allocate-vs-names-fn/test]
// [spec:cg3:sem:tag.cg3.tag.to-u-string-fn/test]
#[test]
fn tag_ctor_rehash_markused_vs_tostring() {
    let mut t = Tag::default();
    t.tag = "x".to_string();
    let base = t.rehash();
    assert_eq!(t.plain_hash, hash_value_ustring("x", 0));
    assert_eq!(base, t.plain_hash, "no flags, no seed: hash == plain_hash");
    t.seed = 5;
    assert_eq!(t.rehash(), t.plain_hash.wrapping_add(5), "seed folded last");
    t.seed = 0;
    t.r#type |= T_CASE_INSENSITIVE;
    let icase_hash = t.rehash();
    assert_ne!(icase_hash, base, "flag markers change the hash");
    assert_ne!(t.r#type & T_SPECIAL, 0, "rehash re-derives T_SPECIAL");

    t.mark_used();
    assert_ne!(t.r#type & T_USED, 0);

    t.allocate_vs_names();
    t.vs_names.as_mut().unwrap().push("n1".to_string());
    t.allocate_vs_names(); // idempotent: does not wipe the existing vector
    assert_eq!(t.vs_names.as_ref().unwrap().len(), 1);
    t.allocate_vs_sets();
    assert!(t.vs_sets.as_ref().unwrap().is_empty());
    t.allocate_vs_sets();
    assert!(t.vs_sets.is_some());

    // toUString: regex tag gets /…/r wrapping; escape mode backslashes specials;
    // a non-empty tag_raw short-circuits everything.
    let mut rt = Tag::default();
    rt.tag = "x".to_string();
    rt.r#type = T_REGEXP;
    assert_eq!(rt.to_u_string(false), "/x/r");
    let mut et = Tag::default();
    et.tag = "a b(c)".to_string();
    assert_eq!(et.to_u_string(true), "a\\ b\\(c\\)");
    assert_eq!(et.to_u_string(false), "a b(c)");
    et.tag_raw = "RAW".to_string();
    assert_eq!(et.to_u_string(true), "RAW");

    // Copy ctor: everything copied except tag_raw (quirk), vs_names cloned.
    t.tag_raw = "orig-raw".to_string();
    let c = t.clone();
    assert_eq!(c.tag, t.tag);
    assert_eq!(c.hash, t.hash);
    assert!(c.tag_raw.is_empty(), "quirk: tag_raw not copied");
    assert_eq!(c.vs_names.as_ref().unwrap(), t.vs_names.as_ref().unwrap());
}

// compare_Tag / equal_Tag / compare_TagVector operator()s (hash-ordered,
// arena-resolved) and fill_tagvector (numeric tags filtered into `did`,
// specials flagged, the rest copied in order).
// [spec:cg3:sem:tag.cg3.compare-tag.operator-fn/test]
// [spec:cg3:sem:tag.cg3.equal-tag.operator-fn/test]
// [spec:cg3:sem:tag.cg3.compare-tag-vector.operator-fn/test]
// [spec:cg3:sem:tag.cg3.fill-tagvector-fn/test]
#[test]
fn tag_comparators_and_fill_tagvector() {
    let mut g = Grammar::default();
    let ta = g.allocate_tag("alpha");
    let tb = g.allocate_tag("beta");
    let (ha, hb) = (g.single_tags_list[ta.0].hash, g.single_tags_list[tb.0].hash);

    assert_eq!(compare_tag(&g, ta, tb), ha < hb);
    assert_eq!(compare_tag(&g, tb, ta), hb < ha);
    assert!(equal_tag(&g, ta, ta));
    assert!(!equal_tag(&g, ta, tb), "distinct interned tags differ by hash");

    // Lexicographic by element hash; prefix ties break on length.
    let va: TagVector = vec![ta];
    let vab: TagVector = vec![ta, tb];
    assert!(compare_tag_vector(&g, &va, &vab), "proper prefix is less");
    assert!(!compare_tag_vector(&g, &vab, &va));
    let vb: TagVector = vec![tb];
    assert_eq!(compare_tag_vector(&g, &va, &vb), ha < hb);

    // fill_tagvector: numeric filtered (did), special flagged, rest pushed.
    let tnum = g.allocate_tag("<n=5>");
    let tspec = g.allocate_tag("spec");
    g.single_tags_list.get_mut(tspec.0).r#type |= T_SPECIAL;
    let input = [tnum, ta, tspec];
    let mut out = TagVector::new();
    let mut did = false;
    let mut special = false;
    fill_tagvector(&g, &input, &mut out, &mut did, &mut special);
    assert!(did, "numeric tag sets did and is dropped");
    assert!(special, "T_SPECIAL tag flags special but is kept");
    assert_eq!(out, vec![ta, tspec], "input order preserved");
}

// ===========================================================================
// ContextualTest.cpp / ContextualTest.hpp
// ===========================================================================

// ContextualTest() defaults, rehash (memoization, field folds, linked
// recursion, seed), operator== (field compare + linked-by-hash special case),
// copy_cntx (shallow field copy leaving is_used/ors untouched).
// [spec:cg3:sem:contextual-test.cg3.contextual-test.contextual-test-fn/test]
// [spec:cg3:sem:contextual-test.cg3.contextual-test.rehash-fn/test]
// [spec:cg3:sem:contextual-test.cg3.contextual-test.operator-fn/test]
// [spec:cg3:sem:contextual-test.cg3.copy-cntx-fn/test]
#[test]
fn contextual_test_ctor_rehash_equals_copy() {
    // Ctor defaults (in-class initializers).
    let d = ContextualTest::default();
    assert_eq!(d.jump_pos, 0, "JUMP_MARK");
    assert!(d.tmpl.is_none() && d.linked.is_none() && d.ors.is_empty());
    assert_eq!((d.offset, d.target, d.hash, d.pos), (0, 0, 0, 0));

    let mut arena: Arena<ContextualTest> = Arena::new();
    let mut ct = ContextualTest::default();
    ct.pos = POS_SPAN_BOTH;
    ct.target = 77;
    ct.offset = -2;
    let a = CtxId(arena.alloc(ct.clone()));
    let h = ContextualTest::rehash(&mut arena, a);
    assert_ne!(h, 0);
    // Memoized: mutating a field after the fact does not change the cache.
    arena[a.0].target = 1234;
    assert_eq!(ContextualTest::rehash(&mut arena, a), h);
    // Seed folds additively on top of the same field hash.
    let mut cts = ct.clone();
    cts.seed = 9;
    let asd = CtxId(arena.alloc(cts));
    assert_eq!(ContextualTest::rehash(&mut arena, asd), h.wrapping_add(9));
    // linked recursion: the child is rehashed and folded in.
    let l = CtxId(arena.alloc(ContextualTest { target: 5, ..Default::default() }));
    let mut ctl = ct.clone();
    ctl.linked = Some(l);
    let b = CtxId(arena.alloc(ctl));
    let hb = ContextualTest::rehash(&mut arena, b);
    assert_ne!(arena[l.0].hash, 0, "linked child rehashed by the recursion");
    assert_ne!(hb, h, "linked hash folded in");

    // equals: identical fields+hash -> true; different linked IDS but equal
    // linked HASHES still count as equal (the C++ special case).
    let l2 = CtxId(arena.alloc(ContextualTest { target: 5, ..Default::default() }));
    ContextualTest::rehash(&mut arena, l2);
    assert_eq!(arena[l.0].hash, arena[l2.0].hash);
    let mut ctl2 = ct.clone();
    ctl2.linked = Some(l2);
    let b2 = CtxId(arena.alloc(ctl2));
    ContextualTest::rehash(&mut arena, b2);
    assert!(arena[b.0].equals(&arena[b2.0], &arena), "linked compared by hash");
    assert!(!arena[b.0].equals(&arena[l.0], &arena), "different tests differ");

    // copy_cntx: fields copied; is_used and ors deliberately untouched.
    let mut trg = ContextualTest::default();
    trg.is_used = true;
    trg.ors.push(CtxId(9));
    let src = &arena[b.0];
    copy_cntx(src, &mut trg);
    assert_eq!(trg.target, src.target);
    assert_eq!(trg.pos, src.pos);
    assert_eq!(trg.linked, src.linked);
    assert_eq!(trg.hash, src.hash);
    assert!(trg.is_used, "is_used NOT copied");
    assert_eq!(trg.ors, vec![CtxId(9)], "ors NOT copied");
}

/// `diff -B`: compare ignoring blank-line differences (same as golden.rs).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn repo_test_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test").join(name)
}

// ContextualTest::markUsed is ported as the PRIVATE `Grammar::context_mark_used`
// (grammar.rs), reachable only through `Grammar::reindex` during a real grammar
// load. Driven by the T_ContextTest fixture: its grammar is full of contextual
// tests, so loading it in vislcg3 runs reindex -> context_mark_used over every
// rule's tests (marking their target/barrier sets used); the run then applies
// those rules, so a correct expected-output diff depends on the marking.
// [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn/test]
#[test]
fn contextual_test_mark_used_via_fixture() {
    let dir = repo_test_dir("T_ContextTest");
    let out = std::env::temp_dir().join(format!("cg3-model-core-ctx-{}.txt", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .arg("-g")
        .arg("grammar.cg3")
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "vislcg3 exited with {status}");
    let got = std::fs::read_to_string(&out).unwrap();
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(diff_b_equal(&want, &got), "T_ContextTest output differs");
}

// ===========================================================================
// SingleWindow.cpp / SingleWindow.hpp
// ===========================================================================

// alloc_swindow (blank window parented to p), appendCohort (local numbering,
// parent wiring, sibling links incl. the cross-window forward/backward
// branches, cohort_map/dep_window registration incl. the [0] alias),
// less_Cohort + compare_Cohort::operator() (local_number, window-number
// tie-break), SingleWindow::clear (teardown + full field reset + relation_map
// prune), ~SingleWindow (teardown + sibling splice), free_swindow (clear +
// slot recycle + handle null).
// [spec:cg3:sem:single-window.cg3.alloc-swindow-fn/test]
// [spec:cg3:sem:single-window.cg3.free-swindow-fn/test]
// [spec:cg3:sem:single-window.cg3.single-window.single-window-fn/test]
// [spec:cg3:sem:single-window.cg3.single-window.clear-fn/test]
// [spec:cg3:sem:single-window.cg3.single-window.append-cohort-fn/test]
// [spec:cg3:sem:single-window.cg3.less-cohort-fn/test]
// [spec:cg3:sem:single-window.cg3.compare-cohort.operator-fn/test]
#[test]
fn single_window_alloc_append_clear_destroy() {
    let mut store = RuntimeStore::new();
    let mut w = Window::new(Some(0));

    // alloc_swindow: parent handle passed through, everything else blank.
    let sw = alloc_swindow(&mut store, Some(3));
    assert_eq!(store.single_windows.get(sw.0).parent, Some(3));
    assert_eq!(store.single_windows.get(sw.0).number, 0);
    store.single_windows.free_slot(sw.0);

    // appendCohort across two linked windows.
    let s1 = w.alloc_append_single_window(&mut store);
    let s2 = w.alloc_append_single_window(&mut store); // s1 <-> s2 siblings
    let mk = |store: &mut RuntimeStore, sw: SwId, gn: u32| {
        let c = alloc_cohort(store, Some(sw));
        store.cohorts.get_mut(c.0).global_number = gn;
        c
    };
    // Append to s2 FIRST so appending into s1 exercises the forward-link branch.
    let cb = mk(&mut store, s2, 10);
    append_cohort(&mut w, &mut store, s2, cb);
    let ca = mk(&mut store, s1, 5);
    append_cohort(&mut w, &mut store, s1, ca);
    assert_eq!(store.cohorts.get(ca.0).local_number, 0);
    assert_eq!(store.cohorts.get(ca.0).parent, Some(s1));
    assert_eq!(store.cohorts.get(ca.0).next, Some(cb), "forward cross-window link");
    assert_eq!(store.cohorts.get(cb.0).prev, Some(ca));
    assert_eq!(w.cohort_map.get(&5), Some(&ca));
    assert_eq!(w.cohort_map.get(&0), Some(&ca), "local 0 aliased at key 0");
    assert_eq!(w.dep_window.get(&10), Some(&cb));
    let ca2 = mk(&mut store, s1, 6);
    append_cohort(&mut w, &mut store, s1, ca2);
    assert_eq!(store.cohorts.get(ca2.0).local_number, 1);
    assert_eq!(store.cohorts.get(ca.0).next, Some(ca2), "backward in-window link");
    assert_eq!(store.cohorts.get(ca2.0).next, Some(cb), "re-linked to next window");

    // less_Cohort / compare_Cohort: local_number first, window number tie-break.
    assert!(less_cohort(&store, ca, ca2), "local 0 < local 1");
    assert!(!less_cohort(&store, ca2, ca));
    assert!(less_cohort(&store, ca, cb), "tie on local 0 -> window 1 < window 2");
    let cmp = compare_Cohort;
    assert!(cmp.call(&store, ca, cb));
    assert!(!cmp.call(&store, cb, ca));

    // clear: relation_map pruned (values <= last cohort's global number),
    // cohorts freed, fields reset.
    w.relation_map.insert((100, 6)); // <= threshold 6 -> pruned
    w.relation_map.insert((200, 7)); // > threshold -> kept
    single_window_clear(&mut w, &mut store, s1);
    assert!(!w.relation_map.contains(100), "stale relation pruned");
    assert!(w.relation_map.contains(200));
    assert!(store.single_windows.get(s1.0).cohorts.is_empty());
    assert_eq!(store.single_windows.get(s1.0).parent, None);
    assert!(store.cohorts.try_get(ca.0).is_none(), "cohorts recycled");
    assert!(!w.cohort_map.contains_key(&5), "clear routed through free_cohort");

    // ~SingleWindow (dtor teardown): splices out of the sibling chain but does
    // NOT free the slot (that is free_swindow's job).
    let a = alloc_swindow(&mut store, Some(0));
    let b = alloc_swindow(&mut store, Some(0));
    let c = alloc_swindow(&mut store, Some(0));
    store.single_windows.get_mut(a.0).next = Some(b);
    store.single_windows.get_mut(b.0).previous = Some(a);
    store.single_windows.get_mut(b.0).next = Some(c);
    store.single_windows.get_mut(c.0).previous = Some(b);
    single_window_destroy(&mut w, &mut store, b);
    assert_eq!(store.single_windows.get(a.0).next, Some(c), "spliced");
    assert_eq!(store.single_windows.get(c.0).previous, Some(a));
    assert!(store.single_windows.try_get(b.0).is_some(), "dtor does not free");

    // free_swindow: clear + recycle + handle null; None is a no-op.
    let mut h = Some(c);
    free_swindow(&mut w, &mut store, &mut h);
    assert_eq!(h, None);
    assert!(store.single_windows.try_get(c.0).is_none());
    let mut none: Option<SwId> = None;
    free_swindow(&mut w, &mut store, &mut none); // must not panic
}

// ===========================================================================
// Window.cpp
// ===========================================================================

// allocSingleWindow (bare alloc + counter numbering), allocPushSingleWindow
// (front of `next`, linked to old front and to `current`),
// allocAppendSingleWindow (back of `next`; quirk: no links when empty), back()
// (next.back / current / previous.back), shuffleWindowsDown (current ->
// previous, next.front -> current), rebuildSingleWindowLinks +
// rebuildCohortLinks (document-order relink), ~Window (destroy: every
// single-window recycled).
// [spec:cg3:sem:window.cg3.window.window-fn/test]
// [spec:cg3:sem:window.cg3.window.alloc-single-window-fn/test]
// [spec:cg3:sem:window.cg3.window.alloc-push-single-window-fn/test]
// [spec:cg3:sem:window.cg3.window.alloc-append-single-window-fn/test]
// [spec:cg3:sem:window.cg3.window.back-fn/test]
// [spec:cg3:sem:window.cg3.window.shuffle-windows-down-fn/test]
// [spec:cg3:sem:window.cg3.window.rebuild-single-window-links-fn/test]
// [spec:cg3:sem:window.cg3.window.rebuild-cohort-links-fn/test]
#[test]
fn window_alloc_shuffle_rebuild_destroy() {
    let mut store = RuntimeStore::new();
    let mut w = Window::new(Some(0));
    assert_eq!(w.back(), None, "empty document");

    // allocSingleWindow: numbered but NOT inserted into any stream.
    let s1 = w.alloc_single_window(&mut store);
    assert_eq!(store.single_windows.get(s1.0).number, 1);
    assert!(w.next.is_empty() && w.current.is_none());

    // allocPushSingleWindow with empty next and no current: no sibling links.
    let s2 = w.alloc_push_single_window(&mut store);
    assert_eq!(store.single_windows.get(s2.0).number, 2);
    assert_eq!(w.next, vec![s2]);
    assert_eq!(store.single_windows.get(s2.0).previous, None);

    // With current set and next non-empty: linked on both sides + front insert.
    w.current = Some(s1);
    let s3 = w.alloc_push_single_window(&mut store);
    assert_eq!(w.next, vec![s3, s2]);
    assert_eq!(store.single_windows.get(s3.0).next, Some(s2));
    assert_eq!(store.single_windows.get(s2.0).previous, Some(s3));
    assert_eq!(store.single_windows.get(s3.0).previous, Some(s1));
    assert_eq!(store.single_windows.get(s1.0).next, Some(s3));

    // allocAppendSingleWindow: linked to the old back, pushed at the end.
    let s4 = w.alloc_append_single_window(&mut store);
    assert_eq!(w.next, vec![s3, s2, s4]);
    assert_eq!(store.single_windows.get(s4.0).previous, Some(s2));
    assert_eq!(store.single_windows.get(s2.0).next, Some(s4));
    assert_eq!(w.back(), Some(s4), "back() prefers next.back()");

    // shuffleWindowsDown: current -> previous, next front -> current.
    w.shuffle_windows_down(&mut store);
    assert_eq!(w.previous, vec![s1]);
    assert_eq!(w.current, Some(s3));
    assert_eq!(w.next, vec![s2, s4]);

    // rebuildSingleWindowLinks: document order previous..current..next.
    w.rebuild_single_window_links(&mut store);
    assert_eq!(store.single_windows.get(s1.0).previous, None);
    assert_eq!(store.single_windows.get(s1.0).next, Some(s3));
    assert_eq!(store.single_windows.get(s3.0).next, Some(s2));
    assert_eq!(store.single_windows.get(s2.0).next, Some(s4));
    assert_eq!(store.single_windows.get(s4.0).next, None);

    // rebuildCohortLinks: cohorts chained across window boundaries.
    let c1 = alloc_cohort(&mut store, Some(s1));
    store.cohorts.get_mut(c1.0).global_number = 1;
    append_cohort(&mut w, &mut store, s1, c1);
    let c2 = alloc_cohort(&mut store, Some(s3));
    store.cohorts.get_mut(c2.0).global_number = 2;
    append_cohort(&mut w, &mut store, s3, c2);
    // Scramble, then rebuild.
    store.cohorts.get_mut(c1.0).next = None;
    store.cohorts.get_mut(c2.0).prev = None;
    w.rebuild_cohort_links(&mut store);
    assert_eq!(store.cohorts.get(c1.0).next, Some(c2), "relinked across windows");
    assert_eq!(store.cohorts.get(c2.0).prev, Some(c1));
    assert_eq!(store.cohorts.get(c1.0).prev, None);
    assert_eq!(store.cohorts.get(c2.0).next, None);

    // ~Window: recycles previous + current + next, nulling current.
    w.destroy(&mut store);
    assert_eq!(w.current, None);
    for s in [s1, s2, s3, s4] {
        assert!(store.single_windows.try_get(s.0).is_none(), "recycled");
    }
}
