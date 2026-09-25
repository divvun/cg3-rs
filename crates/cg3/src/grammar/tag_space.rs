//! What tag parsing and interning need from a grammar, in either phase.
//!
//! Tags are interned into a grammar while it is being loaded
//! ([`GrammarCore`](super::GrammarCore)) and into a run's overlay while a loaded
//! one is applied ([`Grammar`](super::Grammar)). The parser, the relation branch
//! of `parse_tag_raw` and the applicator's `addTag` all walk the same seed
//! probe against whichever of the two they hold. [`TagSpace`] is what that walk
//! reads and writes, and the walk itself is written once, as its provided
//! methods.

use crate::arena::TagId;
use crate::error::ReservedNumber;
use crate::inlines::hash_value_str;
use crate::tag::Tag;

use super::{IcaseTags, RegexTags};

/// A tag arena and the hash index over it — a grammar being loaded, or one
/// run's view of a loaded one.
pub trait TagSpace {
    /// The tag at `id`.
    fn tag(&self, id: TagId) -> &Tag;

    /// C++ `single_tags.find(hash)`: the tag registered at `hash`, if any.
    fn tag_at_hash(&self, hash: u32) -> Option<TagId>;

    /// The regex tags a newly parsed tag's text is tested against.
    fn regex_tags(&self) -> &RegexTags;

    /// The case-insensitive tags a newly parsed tag's text is tested against.
    fn icase_tags(&self) -> &IcaseTags;

    /// Give `tag`, whose seed the probe has settled, a slot; stamp its
    /// `number` (C++ `tag->number = single_tags_list.size() - 1`); and register
    /// it at `hash`.
    fn insert_tag(&mut self, tag: Tag, hash: u32) -> TagId;

    // [spec:cg3:def:grammar.cg3.grammar.add-tag-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-tag-fn]
    /// Interns a `Tag` (by value), deduplicating by hash+text with the 0..9999
    /// seed probe. `t == tag` (pointer identity) can never hold for a fresh
    /// by-value tag, so only the text-equality dedup applies; the read-only
    /// probe is split from the insert so the incoming `tag` moves exactly once
    /// (after the loop).
    ///
    /// The only seed probe in the crate: the applicator's `addTag(Tag*)` and
    /// `parseTagRaw`'s relation interner are this same walk in the C++.
    fn add_tag(&mut self, mut tag: Tag) -> TagId {
        let hash = tag.rehash();
        let mut existing: Option<TagId> = None;
        let mut chosen_seed: Option<u32> = None;
        let mut seed = 0u32;
        while seed < 10000 {
            match self.tag_at_hash(hash.wrapping_add(seed).get()) {
                Some(t_id) => {
                    // C++ `t->tag == tag->tag`: duplicate parked at a seeded slot.
                    // (`hash += seed; return single_tags[hash]` == returning t_id.)
                    if self.tag(t_id).tag == tag.tag {
                        existing = Some(t_id);
                        break;
                    }
                    // else: hash collision, different text — keep probing.
                }
                None => {
                    chosen_seed = Some(seed);
                    break;
                }
            }
            seed += 1;
        }

        if let Some(t_id) = existing {
            // `delete tag` — the incoming value is dropped at end of scope.
            return t_id;
        }

        let seed = chosen_seed.expect("addTag: seed space exhausted");
        // verbosity_level>0 && seed hash-seed warning: deferred I/O.
        tag.seed = seed;
        let new_hash = tag.rehash(); // rehash folds seed → base+seed == ih.
        self.insert_tag(tag, new_hash.get())
    }

    /// The interners' fast path: the tag at `txt`'s un-seeded hash slot, if it
    /// holds exactly `txt`. A miss is not proof of absence — a collision can
    /// have parked the same text at a seeded slot, which
    /// [`add_tag`](Self::add_tag)'s probe finds.
    fn find_unseeded(&self, txt: &str) -> Option<TagId> {
        let tid = self.tag_at_hash(hash_value_str(txt, 0))?;
        let t = self.tag(tid);
        (!t.tag.is_empty() && &*t.tag == txt).then_some(tid)
    }

    /// C++ `new Tag; tag->parseTagRaw(txt, this); addTag(tag)` — no fast path.
    /// A tag carrying a number the hash tables reserve is refused before it
    /// is interned.
    fn add_tag_text(&mut self, txt: &str) -> Result<TagId, ReservedNumber>
    where
        Self: Sized,
    {
        let mut tag = Tag::default();
        crate::tag::parse_tag_raw(&mut tag, txt, self)?;
        Ok(self.add_tag(tag))
    }

    /// `allocateTag`'s body past its checks: the tag already at `txt`'s
    /// un-seeded slot, else a fresh one parsed from `txt` and interned.
    fn intern_text(&mut self, txt: &str) -> Result<TagId, ReservedNumber>
    where
        Self: Sized,
    {
        match self.find_unseeded(txt) {
            Some(tid) => Ok(tid),
            None => self.add_tag_text(txt),
        }
    }
}
