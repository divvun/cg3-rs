//! A loaded grammar as one run holds it.
//!
//! [`GrammarCore`](super::GrammarCore) is the grammar: the loaders build it,
//! the writers serialise it, and once it is loaded it goes behind an `Arc` and
//! nothing edits it again. This module is the run's side — [`Grammar`], which
//! pairs a shared core with the tag state one run owns, and the two spanning
//! views ([`TagStore`], [`TagIndex`]) that let a `TagId` keep naming one flat
//! space across both.
//!
//! See the crate-level note in [`super`] for why the split exists.

use std::ops::{Deref, Index};
use std::sync::Arc;

use crate::arena::TagId;
use crate::flat_unordered_map::FlatUnorderedMap;
use crate::tag::{Tag, TagType};

use super::{GrammarCore, IcaseTags, RegexTags, TagSpace};

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

/// The tag arena AS ONE RUN SEES IT: the core's tags at their own ids, this
/// run's interned tags at the ids above them.
///
/// `TagId(i)` resolves to the core for `i < core.single_tags_list.capacity()`
/// and to the run's own `Vec` above that, so a `TagId` still names one flat
/// space and `single_tags_list[id]` reads the same everywhere it always did.
/// The core never grows, so the boundary is fixed and no core id ever moves.
///
/// There is deliberately no `IndexMut`: a `&mut Tag` that might land in the
/// core is not expressible, because another pipeline may be reading that tag.
pub struct TagStore {
    core: Arc<GrammarCore>,
    /// The tags THIS RUN interned, in id order from the core's length up. A
    /// plain `Vec`, not an [`Arena`](crate::arena::Arena): a run never frees a
    /// tag it interned.
    run: Vec<Tag>,
}

impl TagStore {
    /// The shared half.
    #[inline]
    pub fn core(&self) -> &GrammarCore {
        &self.core
    }

    /// Where the core half ends and this run's own tags begin.
    #[inline]
    fn core_len(&self) -> u32 {
        self.core.single_tags_list.capacity()
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
            self.core.single_tags_list.get(i)
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
            self.core.single_tags_list.try_get(i)
        } else {
            self.run.get((i - core_len) as usize)
        }
    }

    /// Give `tag` the next slot above everything in the span and stamp its
    /// `number` (C++ `tag->number = single_tags_list.size() - 1`).
    fn intern(&mut self, mut tag: Tag) -> TagId {
        let idx = self.capacity();
        tag.number = idx;
        self.run.push(tag);
        TagId(idx)
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
/// as it does on the core. The members below SHADOW their core counterparts on
/// purpose: naming `regex_tags` through a `Grammar` has to mean the run's, or a
/// run would scan a set missing its own additions.
///
/// There is no `DerefMut`, so a run cannot edit the grammar it is applying,
/// and that is checked by the compiler rather than at run time:
///
/// ```compile_fail
/// let mut grammar = cg3::grammar::Grammar::default();
/// grammar.has_dep = true;
/// ```
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
    /// what the binary writer serialises.
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
    /// A run over an empty grammar — what a tool holds before it has loaded
    /// the real one.
    fn default() -> Self {
        Grammar::from_core(Arc::new(GrammarCore::default()))
    }
}

/// A run over a grammar nobody else is applying — `from_core` over a fresh
/// `Arc`.
impl From<GrammarCore> for Grammar {
    fn from(core: GrammarCore) -> Self {
        Grammar::from_core(Arc::new(core))
    }
}

impl Deref for Grammar {
    type Target = GrammarCore;
    #[inline]
    fn deref(&self) -> &GrammarCore {
        &self.single_tags_list.core
    }
}

/// The hash → tag index across both halves — the C++ `Grammar::single_tags`,
/// which a run both reads and adds to.
///
/// Built per lookup by [`Grammar::single_tags`] (and, over the core alone, by
/// [`GrammarCore::single_tags`]); holds no state of its own.
pub struct TagIndex<'a> {
    pub(super) core: &'a FlatUnorderedMap<u32, TagId>,
    pub(super) run: Option<&'a FlatUnorderedMap<u32, TagId>>,
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
        if let Some(run) = self.run {
            let it = run.find(hash);
            if it != run.end() {
                return TagHashRef(Some(*it.get()));
            }
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
        self.core.size() + self.run.map_or(0, |run| run.size())
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

    /// The entry's tag, or `None` for `end()`.
    #[inline]
    pub fn tag(self) -> Option<TagId> {
        self.0.map(|(_, id)| id)
    }
}

impl Grammar {
    /// A run over `core` — the constructor the whole split exists for. A host
    /// loads ONE grammar, puts it behind an `Arc`, and builds a pipeline per
    /// worker from clones of it; N workers then cost one grammar plus N
    /// overlays instead of N grammars.
    ///
    /// The overlay starts from the core's load-time tag flags and copies of its
    /// regex / case-insensitive tag sets. Nothing is shared with any other run
    /// over the same core, so neither can see the other's interned tags or flag
    /// changes.
    pub fn from_core(core: Arc<GrammarCore>) -> Grammar {
        let tag_flags = (0..core.single_tags_list.capacity())
            .map(|i| {
                core.single_tags_list
                    .try_get(i)
                    .map_or(TagType::empty(), |t| t.r#type)
            })
            .collect();
        let regex_tags = core.regex_tags.clone();
        let icase_tags = core.icase_tags.clone();
        Grammar {
            single_tags_list: TagStore {
                core,
                run: Vec::new(),
            },
            single_tags_run: FlatUnorderedMap::default(),
            tag_flags,
            regex_tags,
            icase_tags,
        }
    }

    /// The core this run applies.
    #[inline]
    pub fn core(&self) -> &GrammarCore {
        &self.single_tags_list.core
    }

    /// Another handle on the core, for a second run to apply the same grammar
    /// without rebuilding it.
    pub fn shared_core(&self) -> Arc<GrammarCore> {
        Arc::clone(&self.single_tags_list.core)
    }

    /// C++ `Grammar::single_tags` — the hash → tag index, spanning the core's
    /// load-time entries and this run's own.
    #[inline]
    pub fn single_tags(&self) -> TagIndex<'_> {
        TagIndex {
            core: &self.single_tags_list.core.tags_by_hash,
            run: Some(&self.single_tags_run),
        }
    }

    /// The type flags THIS RUN sees for `tag` — read `Grammar::tag_flags`,
    /// never `Tag::r#type`, from any code that runs while a stream is being
    /// applied.
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
}

/// A run interns into its own half: the tag goes above the core, its flags
/// start as `parse_tag_raw` left them, and its hash entry goes to the run's map.
impl TagSpace for Grammar {
    #[inline]
    fn tag(&self, id: TagId) -> &Tag {
        self.single_tags_list.get(id.0)
    }

    #[inline]
    fn tag_at_hash(&self, hash: u32) -> Option<TagId> {
        self.single_tags().find(hash).tag()
    }

    fn regex_tags(&self) -> &RegexTags {
        &self.regex_tags
    }

    fn icase_tags(&self) -> &IcaseTags {
        &self.icase_tags
    }

    fn insert_tag(&mut self, tag: Tag, hash: u32) -> TagId {
        let ty = tag.r#type;
        let id = self.single_tags_list.intern(tag);
        self.tag_flags.push(ty);
        self.single_tags_run.insert((hash, id));
        id
    }
}
