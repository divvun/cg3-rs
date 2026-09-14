//! The seam between a loaded grammar and the run applying it.
//!
//! [`GrammarCore`](super::GrammarCore) is everything the load produced and no
//! run may change; this module is the other half — [`Grammar`], which pairs a
//! core with the tag state one run owns, and the two spanning views
//! ([`TagStore`], [`TagIndex`]) that let a `TagId` keep naming one flat space
//! across both.
//!
//! See the crate-level note in [`super`] for why the split exists.

use std::ops::{Deref, DerefMut, Index};
use std::sync::Arc;

use crate::arena::TagId;
use crate::flat_unordered_map::FlatUnorderedMap;
use crate::sorted_vector::SortedVector;
use crate::tag::{Tag, TagType};

use super::{GrammarCore, IcaseTags, RegexTags};

/// The whole point of the split, as a bound the compiler checks rather than a
/// claim in a comment: a [`GrammarCore`] can cross threads and be read from
/// several at once, so one `Arc<GrammarCore>` can serve every pipeline in a
/// process.
const _: () = {
    fn shareable<T: Send + Sync>() {}
    fn proof() {
        shareable::<GrammarCore>();
        shareable::<Arc<GrammarCore>>();
    }
    let _ = proof;
};

/// How a [`Grammar`] holds its [`GrammarCore`].
///
/// One handle, two states, because a grammar is mutable exactly once: while it
/// is being loaded.
enum CoreHandle {
    /// Still loading. This grammar is the core's sole owner and edits it in
    /// place — no atomics, no checks, no sharing.
    Building(Box<GrammarCore>),
    /// Loaded. The core is immutable in intent and shareable in fact. Editing
    /// one that is still unshared remains possible (the grammar writers rename
    /// sets after a run, and `main` still owns the only handle); editing a
    /// SHARED one is the bug this whole split exists to make impossible, and
    /// panics instead of corrupting another pipeline's grammar.
    Frozen(Arc<GrammarCore>),
}

/// The tag arena AS ONE RUN SEES IT: the frozen core's tags at their own ids,
/// this run's interned tags at the ids above them.
///
/// `TagId(i)` resolves to the core for `i < core.single_tags_list.capacity()`
/// and to the run's own `Vec` above that, so a `TagId` still names one flat
/// space and `single_tags_list[id]` reads the same everywhere it always did.
/// The core arena never grows once frozen, so the boundary is fixed and the
/// ids of core tags never move.
///
/// There is deliberately no `IndexMut`: a `&mut Tag` that might land in the
/// core is not expressible, because another pipeline may be reading that tag.
/// `intern` is the run's only way to add one, and the load-time editors
/// ([`building_mut`](Self::building_mut), [`put_building`](Self::put_building))
/// say in their names that they only work before the core is frozen.
///
/// This type also HOLDS the core handle. `single_tags_list` is the one field
/// that has to see both halves at once, Rust fields cannot borrow their
/// siblings, and a second `Arc` clone anywhere inside `Grammar` would take the
/// in-place edit away from the loaders forever. [`Grammar::core`] reads it back
/// out, and [`Grammar`]'s `Deref` goes through it.
pub struct TagStore {
    core: CoreHandle,
    /// The tags THIS RUN interned, in id order from `core_len` up. A plain
    /// `Vec`, not an [`Arena`]: a run never frees a tag it interned.
    run: Vec<Tag>,
}

impl Default for TagStore {
    fn default() -> Self {
        TagStore {
            core: CoreHandle::Building(Box::default()),
            run: Vec::new(),
        }
    }
}

impl TagStore {
    /// The frozen (or still-building) half of the grammar.
    #[inline]
    pub fn core(&self) -> &GrammarCore {
        match &self.core {
            CoreHandle::Building(c) => c,
            CoreHandle::Frozen(c) => c,
        }
    }

    /// The core, mutably. Panics once the core is SHARED — see `CoreHandle`.
    #[inline]
    pub fn core_mut(&mut self) -> &mut GrammarCore {
        match &mut self.core {
            CoreHandle::Building(c) => c,
            CoreHandle::Frozen(c) => Arc::get_mut(c).expect(
                "a frozen grammar core is shared between runs and cannot be edited; \
                 the run's own tag state lives in the overlay beside it",
            ),
        }
    }

    /// Where the core half ends and this run's own tags begin.
    #[inline]
    fn core_len(&self) -> u32 {
        self.core().single_tags_list.capacity()
    }

    /// Whether new tags belong to the run rather than to the grammar.
    #[inline]
    fn is_frozen(&self) -> bool {
        matches!(self.core, CoreHandle::Frozen(_))
    }

    /// `Arena::capacity` over the span: highest id + 1 across both halves.
    #[inline]
    pub fn capacity(&self) -> u32 {
        self.core_len() + self.run.len() as u32
    }

    /// `Arena::get` over the span.
    #[inline]
    pub fn get(&self, i: u32) -> &Tag {
        let core_len = self.core_len();
        if i < core_len {
            self.core().single_tags_list.get(i)
        } else {
            &self.run[(i - core_len) as usize]
        }
    }

    /// `Arena::try_get` over the span. A run never frees a tag it interned, so
    /// only the core half can answer `None` for an in-range id.
    #[inline]
    pub fn try_get(&self, i: u32) -> Option<&Tag> {
        let core_len = self.core_len();
        if i < core_len {
            self.core().single_tags_list.try_get(i)
        } else {
            self.run.get((i - core_len) as usize)
        }
    }

    /// Give `tag` a slot and stamp its `number` (C++ `tag->number =
    /// single_tags_list.size() - 1`), returning its id.
    ///
    /// Before [`Grammar::freeze`] that slot is in the core, because the grammar
    /// is still being built; after it, in the run's own half.
    pub(super) fn intern(&mut self, mut tag: Tag) -> TagId {
        if self.is_frozen() {
            let idx = self.capacity();
            tag.number = idx;
            self.run.push(tag);
            TagId(idx)
        } else {
            let core = self.core_mut();
            let idx = core.single_tags_list.alloc(tag);
            core.single_tags_list.get_mut(idx).number = idx;
            TagId(idx)
        }
    }

    /// Edit a tag of a grammar that is still being LOADED — `reindex`'s
    /// `T_TEXTUAL` / `T_USED` / `T_MAPPING` passes and nothing else.
    ///
    /// Panics on a frozen core: from that point a tag is shared, and a run that
    /// needs different flags has its own copy of them
    /// ([`Grammar::tag_type_insert`]).
    pub fn building_mut(&mut self, i: u32) -> &mut Tag {
        assert!(
            !self.is_frozen(),
            "tag {i} belongs to a frozen grammar core and cannot be edited; \
             a run's view of a tag's type lives in Grammar::tag_flags"
        );
        self.core_mut().single_tags_list.get_mut(i)
    }

    /// Place a whole tag at an already-allocated slot (the binary loader reads
    /// records out of order and puts each at its own `number`). Load-time only,
    /// like [`building_mut`](Self::building_mut).
    pub fn put_building(&mut self, i: u32, tag: Tag) {
        assert!(
            !self.is_frozen(),
            "tag {i} belongs to a frozen grammar core and cannot be replaced"
        );
        self.core_mut().single_tags_list[i] = tag;
    }

    /// Reserve a slot while loading (the binary loader pre-sizes the arena).
    pub fn alloc_building(&mut self, tag: Tag) -> u32 {
        assert!(
            !self.is_frozen(),
            "a frozen grammar core cannot grow; a run's tags go to the overlay"
        );
        self.core_mut().single_tags_list.alloc(tag)
    }

    /// `Arena::free_slot` — `destroy_tag`'s backing, and load-time only for the
    /// same reason as [`building_mut`](Self::building_mut).
    pub(super) fn free_slot(&mut self, i: u32) {
        assert!(
            !self.is_frozen(),
            "tag {i} belongs to a frozen grammar core and cannot be freed"
        );
        self.core_mut().single_tags_list.free_slot(i);
    }

    /// Finish loading: the core becomes immutable-in-intent and shareable, and
    /// every tag interned from here on is the RUN's. Idempotent.
    fn freeze(&mut self) {
        if let CoreHandle::Building(core) = &mut self.core {
            let core = std::mem::take(core);
            self.core = CoreHandle::Frozen(Arc::from(core));
        }
    }
}

impl Index<u32> for TagStore {
    type Output = Tag;
    #[inline]
    fn index(&self, i: u32) -> &Tag {
        self.get(i)
    }
}

/// A loaded grammar as one run holds it: the shared [`GrammarCore`] plus the
/// tag state that run owns.
///
/// Derefs to the core, so every field the load produced — sets, rules,
/// contexts, the reindex indexes, the mode flags — reads through a `Grammar`
/// exactly as it did when they were all one struct. The five members below
/// SHADOW their core counterparts on purpose: naming `regex_tags` through a
/// `Grammar` has to mean the run's, or a run would scan a set missing its own
/// additions.
pub struct Grammar {
    /// Both halves of the tag arena, and the handle to the core (see
    /// [`TagStore`]).
    pub single_tags_list: TagStore,

    /// The hash entries THIS RUN added — the upper half of the span
    /// [`single_tags`](Self::single_tags) reads. Private because a lookup that
    /// saw only these would miss every tag the grammar was compiled with.
    single_tags_run: FlatUnorderedMap<u32, TagId>,

    /// The tag type flags AS THE RUN SEES THEM, dense and parallel to
    /// [`single_tags_list`](Self::single_tags_list) (same index, same length).
    ///
    /// The C++ keeps one `uint32_t type` inside each `Tag` and writes it while
    /// applying: `T_TEXTUAL` when a runtime-interned regex/icase tag makes
    /// existing tags textual, `T_MAPPING` when a mapped tag turns out to have
    /// been deduped onto a pre-existing one, and a `T_MAPPING` CLEAR per binary
    /// window. Those are properties of the STREAM being applied, not of the
    /// grammar, and a `Tag` is otherwise an immutable record shared by every
    /// pipeline in the process.
    ///
    /// So the run gets its own copy. Every runtime read goes through
    /// [`tag_type`](Self::tag_type) and every runtime write through
    /// [`tag_type_insert`](Self::tag_type_insert) /
    /// [`tag_type_remove`](Self::tag_type_remove); `Tag::r#type` keeps the
    /// LOAD-TIME value, which is what `rehash` folds into the tag's identity and
    /// what the binary writer serialises. Load-time code (the parsers,
    /// [`reindex`](Self::reindex), the relabeller, the writer, `Tag`'s own
    /// methods) reads `Tag::r#type` directly — it runs before this exists.
    ///
    /// Dense, not a delta map: it is read on the hottest path in the engine, and
    /// 4 bytes per tag is ~56 KB for a 14k-tag grammar against a grammar of
    /// hundreds of megabytes.
    tag_flags: Vec<TagType>,

    /// The regex tags this run works from: the core's, plus any it interned.
    /// Seeded wholesale rather than spanned — there are a handful of them
    /// against a whole grammar, and the insert here answers "was it already
    /// known?", which a two-level set would have to answer twice.
    pub regex_tags: RegexTags,
    /// The case-insensitive tags this run works from; see
    /// [`regex_tags`](Self::regex_tags).
    pub icase_tags: IcaseTags,
}

impl Default for Grammar {
    /// A grammar with an empty core, still building — the state every loader
    /// starts from.
    fn default() -> Self {
        Grammar {
            single_tags_list: TagStore::default(),
            single_tags_run: FlatUnorderedMap::default(),
            tag_flags: Vec::new(),
            regex_tags: RegexTags::default(),
            icase_tags: SortedVector::new(),
        }
    }
}

impl Deref for Grammar {
    type Target = GrammarCore;
    #[inline]
    fn deref(&self) -> &GrammarCore {
        self.single_tags_list.core()
    }
}

impl DerefMut for Grammar {
    #[inline]
    fn deref_mut(&mut self) -> &mut GrammarCore {
        self.single_tags_list.core_mut()
    }
}

/// The hash → tag index across both halves — the C++ `Grammar::single_tags`,
/// which a run both reads and adds to.
///
/// Built per lookup by [`Grammar::single_tags`]; holds no state of its own.
pub struct TagIndex<'a> {
    core: &'a FlatUnorderedMap<u32, TagId>,
    run: &'a FlatUnorderedMap<u32, TagId>,
}

impl TagIndex<'_> {
    /// `single_tags.find(hash)`. The two halves hold DISJOINT keys — the
    /// interners only ever insert a key neither half already answers for — so
    /// consulting the core first and the run second sees exactly what one map
    /// would have.
    ///
    /// That disjointness is what keeps the seed probe honest. `addTag` walks
    /// `hash + seed` upwards and stops at the first key nothing answers for; a
    /// probe that consulted the run alone would find `hash + 0` free and mint a
    /// second id for text the core already has, which would silently change
    /// what matches what.
    #[inline]
    pub fn find(&self, hash: u32) -> TagHashRef {
        let it = self.core.find(hash);
        if it != self.core.end() {
            return TagHashRef(Some(*it.get()));
        }
        let it = self.run.find(hash);
        if it != self.run.end() {
            return TagHashRef(Some(*it.get()));
        }
        TagHashRef(None)
    }

    /// `single_tags.end()` — the not-found answer to compare a
    /// [`find`](Self::find) against.
    #[inline]
    pub fn end(&self) -> TagHashRef {
        TagHashRef(None)
    }

    /// `single_tags.size()` — live entries across both halves.
    pub fn size(&self) -> usize {
        self.core.size() + self.run.size()
    }
}

/// What [`TagIndex::find`] answers with: the `(hash, tag)` entry, or nothing.
///
/// Stands in for the C++ `single_tags` iterator, which the call sites only ever
/// compared against `end()` and dereferenced.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct TagHashRef(Option<(u32, TagId)>);

impl TagHashRef {
    /// `*it` — the entry. Dereferencing `end()` is the C++ null deref, and
    /// panics here rather than reading past the map.
    #[inline]
    pub fn get(self) -> (u32, TagId) {
        self.0
            .expect("single_tags: dereferenced a past-the-end iterator")
    }
}

impl Grammar {
    /// `single_tags[hash] = id` — register an interned tag under the hash the
    /// probe settled on, in whichever half the tag itself went to.
    pub(crate) fn insert_tag_hash(&mut self, hash: u32, id: TagId) {
        if self.single_tags_list.is_frozen() {
            self.single_tags_run.insert((hash, id));
        } else {
            self.single_tags_list
                .core_mut()
                .tags_by_hash
                .insert((hash, id));
        }
    }

    /// C++ `Grammar::single_tags` — the hash → tag index, spanning the core's
    /// load-time entries and this run's own.
    #[inline]
    pub fn single_tags(&self) -> TagIndex<'_> {
        TagIndex {
            core: &self.single_tags_list.core().tags_by_hash,
            run: &self.single_tags_run,
        }
    }

    /// The core, shared or not.
    #[inline]
    pub fn core(&self) -> &GrammarCore {
        self.single_tags_list.core()
    }

    /// The core, mutably — the explicit form of the `DerefMut` every
    /// `grammar.some_field = …` already goes through, for the places that need
    /// two of its fields borrowed at once.
    #[inline]
    pub fn core_mut(&mut self) -> &mut GrammarCore {
        self.single_tags_list.core_mut()
    }

    /// Finish loading: the core becomes shareable, and every tag interned from
    /// here on belongs to the RUN rather than to the grammar. Idempotent.
    ///
    /// Called where a run begins (`GrammarApplicator::set_grammar`), not where
    /// a load ends: a relabelled grammar goes on gaining tags after its last
    /// `reindex`, and those are the grammar's — they have to serialise.
    pub fn freeze(&mut self) {
        self.single_tags_list.freeze();
    }

    /// Freeze and hand out the core, for a second run to apply the same grammar
    /// without rebuilding it.
    ///
    /// The handle this whole split exists to produce. Note that it makes the
    /// core SHARED: editing it through this `Grammar` panics from here on.
    pub fn shared_core(&mut self) -> Arc<GrammarCore> {
        self.freeze();
        match &self.single_tags_list.core {
            CoreHandle::Frozen(c) => Arc::clone(c),
            CoreHandle::Building(_) => unreachable!("just frozen"),
        }
    }

    /// The type flags THIS RUN sees for `tag` — read
    /// `Grammar::tag_flags`, never `Tag::r#type`, from any code that
    /// runs while a stream is being applied.
    ///
    /// The only thing in the engine that knows where the run's flags live. When
    /// they move to a per-run overlay, this moves with them and its callers do
    /// not.
    #[inline]
    pub fn tag_type(&self, tag: TagId) -> TagType {
        self.tag_flags[tag.0 as usize]
    }

    /// `tag->type |= bits` for the RUN (C++ writes the grammar's own `Tag`).
    #[inline]
    pub fn tag_type_insert(&mut self, tag: TagId, bits: TagType) {
        self.tag_flags[tag.0 as usize] |= bits;
    }

    /// `tag->type &= ~bits` for the RUN (C++ writes the grammar's own `Tag`).
    #[inline]
    pub fn tag_type_remove(&mut self, tag: TagId, bits: TagType) {
        self.tag_flags[tag.0 as usize] &= !bits;
    }

    /// Seed `tag_flags[tag]` from the tag's load-time flags, growing the array
    /// to cover the slot. Called for every tag the interner allocates, so the
    /// array stays parallel to the arena — including for tags interned during a
    /// run, whose flags start out exactly as `parse_tag_raw` left them.
    pub(super) fn record_tag_flags(&mut self, tag: TagId) {
        let ty = self.single_tags_list[tag.0].r#type;
        let i = tag.0 as usize;
        if i >= self.tag_flags.len() {
            self.tag_flags.resize(i + 1, TagType::empty());
        }
        self.tag_flags[i] = ty;
    }

    /// Copy the tag state a run owns out of the core it was just built from:
    /// every tag's load-time flags into `tag_flags`, and the
    /// core's regex / case-insensitive tag sets into the run's.
    ///
    /// Run at the end of [`reindex`](Self::reindex), which is the last thing to
    /// touch `Tag::r#type` before a grammar is applied: the parsers build tags
    /// through `add_tag` (which seeds each slot as it goes), the binary reader
    /// writes arena slots directly, and `reindex` itself rewrites `T_TEXTUAL`,
    /// `T_USED` and `T_MAPPING` over the whole arena and fills `regex_tags` /
    /// `icase_tags`. Re-seeding wholesale here absorbs all of it, so no loader
    /// has to remember to — and it happens whether or not the grammar is ever
    /// frozen, so an unfrozen one (relabelling, `cg-comp`) reads the same sets
    /// it always did.
    pub fn materialise_run_tag_state(&mut self) {
        let cap = self.single_tags_list.capacity();
        self.tag_flags.clear();
        self.tag_flags.resize(cap as usize, TagType::empty());
        for i in 0..cap {
            if let Some(t) = self.single_tags_list.try_get(i) {
                self.tag_flags[i as usize] = t.r#type;
            }
        }
        self.regex_tags = self.core().regex_tags.clone();
        self.icase_tags = self.core().icase_tags.clone();
    }
}
