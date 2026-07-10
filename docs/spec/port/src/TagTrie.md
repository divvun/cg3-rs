# src/TagTrie.hpp

> [spec:cg3:def:tag-trie.cg3.trie-copy-fn]
> inline trie_t trie_copy(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-copy-fn]
> Deep-copies a whole trie and returns a new trie_t by value. A trie_t is
> a bc::flat_map<Tag*, trie_node_t, compare_Tag>, i.e. a sorted vector map
> keyed by Tag* ordered ascending by Tag->hash. Creates a local trie_t nt;
> for each pair p iterated in ascending-Tag-hash order, sets
> nt[p.first].terminal = p.second.terminal (flat_map operator[] creating
> the node) and, when p.second.trie is non-null, assigns
> nt[p.first].trie = _trie_copy_helper(*p.second.trie) to recursively deep-
> copy the sub-trie. The Tag* keys are shared (pointers copied, Tags not
> cloned); only node structure and terminal flags are duplicated. Returns
> nt.

> [spec:cg3:def:tag-trie.cg3.trie-copy-helper-fn]
> inline std::unique_ptr<trie_t> _trie_copy_helper(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-copy-helper-fn]
> Deep-copies a sub-trie and returns it as std::unique_ptr<trie_t>.
> Allocates a new empty trie_t. For each pair p in the source (ascending
> Tag-hash order) sets (*nt)[p.first].terminal = p.second.terminal
> (creating the node) and, if p.second.trie is non-null, recursively sets
> (*nt)[p.first].trie = _trie_copy_helper(*p.second.trie). Tag* keys are
> shared, not cloned; node structure and terminal flags are duplicated.
> Returns the new owning pointer. This is the recursive worker used by
> trie_copy for nested levels.

> [spec:cg3:def:tag-trie.cg3.trie-delete-fn]
> inline void trie_delete(trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-delete-fn]
> Recursively tears down all descendant sub-tries of trie. For each pair
> (ascending Tag-hash order), if its node has a non-null sub-trie, recurse
> trie_delete into it and then reset() (free) that unique_ptr. It does NOT
> clear the passed-in map's own entries — the top-level keys and their
> terminal flags remain, only their .trie child pointers are freed and
> nulled. (unique_ptr destruction would free these anyway; this is an
> explicit depth-first prune.) Returns nothing.

> [spec:cg3:def:tag-trie.cg3.trie-get-tag-list-fn]
> inline bool trie_getTagList(const trie_t& trie, TagList& theTags, const void* node)

> [spec:cg3:sem:tag-trie.cg3.trie-get-tag-list-fn]
> Depth-first search that reconstructs the tag path leading to a specific
> node, returning bool found. theTags is an in/out accumulator holding the
> current path. For each kv (ascending Tag-hash order): push_back kv.first
> onto theTags; if node == &kv (the target void* equals the address of
> this key/value pair inside the flat_map) return true immediately, with
> theTags holding the full path from root to and including this tag; else
> if kv.second.trie is non-null and the recursive call trie_getTagList(
> *kv.second.trie, theTags, node) returns true, return true (theTags left
> intact); else pop_back (backtrack) and continue to the next sibling.
> Returns false if the node is not found in this subtree, with theTags
> restored to its entry state. Note the target is matched by pointer
> identity of the flat_map value pair, not by tag value.

> [spec:cg3:def:tag-trie.cg3.trie-get-tags-fn]
> inline TagVectorSet trie_getTags(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-get-tags-fn]
> Collects the set of complete tag-sequences (each a root-to-terminal
> path), where each sequence is individually SORTED by compare_Tag
> (ascending Tag->hash) before insertion, into a TagVectorSet (a
> std::set<TagVector, compare_TagVector>, so equivalent sequences merge).
> Public one-arg form: for each top-level kv in ascending Tag-hash order,
> start a fresh TagVector tv = [kv.first]; if kv.second.terminal, std::sort
> tv with compare_Tag, insert a copy into rv, pop_back, and continue; else
> if kv.second.trie is non-null, recurse into the shared-tv helper
> trie_getTags(*sub, rv, tv) which extends tv one level deeper and applies
> the same terminal/sort/insert/pop_back logic. Returns rv. Terminal nodes
> never also carry a sub-trie (trie_insert clears .trie when setting
> terminal), so the terminal branch's `continue` loses nothing.
> BUG/QUIRK to reproduce bug-for-bug: in the shared-tv recursion, upon a
> terminal the code std::sorts tv IN PLACE and then pop_back removes the
> LAST element of the now-sorted vector — that is the highest-hash tag, not
> necessarily the tag just pushed. Whenever the pushed tag is not the
> max-hash element, this corrupts the shared prefix used by subsequent
> siblings at that level, so wrong tag combinations get inserted. The port
> must replicate this (sort-then-pop-last) exactly.

> [spec:cg3:def:tag-trie.cg3.trie-get-tags-ordered-fn]
> inline TagVectorSet trie_getTagsOrdered(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-get-tags-ordered-fn]
> Like trie_getTags but WITHOUT any sorting: collects root-to-terminal
> paths preserving their in-trie order (ascending Tag-hash at each level)
> into a TagVectorSet (std::set ordered by compare_TagVector). Public form:
> for each top-level kv (ascending Tag-hash order) start a fresh TagVector
> tv = [kv.first]; if kv.second.terminal, insert tv into rv, pop_back,
> continue; else if kv.second.trie is non-null, recurse the shared-tv
> helper trie_getTagsOrdered(*sub, rv, tv) that extends tv deeper. Because
> tv is never reordered, pop_back always removes the just-pushed tag, so
> backtracking is correct here (unlike the sort-then-pop bug in
> trie_getTags). Returns rv.

> [spec:cg3:def:tag-trie.cg3.trie-has-type-fn]
> inline bool trie_hasType(trie_t& trie, uint32_t type)

> [spec:cg3:sem:tag-trie.cg3.trie-has-type-fn]
> Depth-first search returning true iff any Tag anywhere in the trie has
> any bit of the `type` mask set in its own `type` bitmask. For each kv
> (ascending Tag-hash order): if (kv.first->type & type) is nonzero return
> true; else if kv.second.trie is non-null and trie_hasType(*sub, type)
> returns true, return true. After exhausting all entries at this level,
> return false.

> [spec:cg3:def:tag-trie.cg3.trie-insert-fn]
> inline bool trie_insert(trie_t& trie, const TagVector& tv, size_t w = 0)

> [spec:cg3:sem:tag-trie.cg3.trie-insert-fn]
> Inserts the tag sequence tv into the trie, recursing from depth w
> (default 0); returns bool. trie is a bc::flat_map<Tag*, trie_node_t,
> compare_Tag> keyed by Tag* and kept sorted ascending by Tag->hash. Steps:
> node = trie[tv[w]] — flat_map operator[] returns the node for key tv[w],
> default-inserting {terminal=false, trie=null} if the key is absent. If
> node.terminal is already true, return false (a shorter sequence already
> terminates at this prefix; refuse to insert). If w < tv.size()-1 (not yet
> the last tag): if node.trie is null, allocate a new empty trie_t into it,
> then return trie_insert(*node.trie, tv, w+1) (recurse to the next depth).
> Otherwise (w is the last index, tv.size()-1): set node.terminal = true,
> call node.trie.reset() to DELETE any existing sub-trie (discarding all
> longer sequences that branched from here), and return true. So it returns
> true when a new terminal is established, false when blocked by an existing
> shorter terminal. QUIRK: inserting a sequence that is a proper prefix of
> existing longer ones wipes those longer branches. EDGE: an empty tv makes
> tv.size()-1 underflow to SIZE_MAX and tv[0] an out-of-bounds access (UB);
> callers must pass a non-empty vector. Because compare_Tag orders by hash
> alone, two distinct Tag* with equal hash collide as one trie key.

> [spec:cg3:def:tag-trie.cg3.trie-markused-fn]
> inline void trie_markused(trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-markused-fn]
> Recursively marks every Tag in the trie as used. For each kv in ascending
> Tag-hash order, calls kv.first->markUsed() and, if kv.second.trie is
> non-null, recurses trie_markused(*sub). Side effect only (mutates the Tag
> objects' used state); returns nothing.

> [spec:cg3:def:tag-trie.cg3.trie-node-t]
> struct trie_node_t {
>   bool terminal = false;
>   std::unique_ptr<trie_t> trie;
> }

> [spec:cg3:def:tag-trie.cg3.trie-rehash-fn]
> inline uint32_t trie_rehash(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-rehash-fn]
> Computes a uint32_t structural hash of the trie (used for stable grammar
> hashing). Initializes retval = 0. For each kv in ascending Tag-hash order:
> retval = hash_value(kv.first->hash, retval) — folds the tag's precomputed
> 32-bit hash into the running value using CG3's uint32 hash_value(c, h),
> which behaves as: if h == 0 set h = CG3_HASH_SEED (705577479); then
> h = c + (h << 6) + (h << 16) - h (all uint32 wraparound); then if h is 0,
> 0xFFFFFFFF, or 0xFFFFFFFE, reset h = CG3_HASH_SEED; return h. If
> kv.second.trie is non-null, additionally fold the sub-trie:
> retval = hash_value(trie_rehash(*kv.second.trie), retval). Return retval.
> The very first fold uses seed CG3_HASH_SEED because retval starts at 0.
> PARITY NOTE: terminal flags are NOT hashed — only Tag hashes and the
> nesting structure (via recursion order = ascending Tag hash) contribute,
> so two tries differing only in terminal markers rehash identically. The
> Rust port must use uint32 wrapping arithmetic and this exact seed/guard.

> [spec:cg3:def:tag-trie.cg3.trie-serialize-fn]
> inline void trie_serialize(const trie_t& trie, std::ostream& out)

> [spec:cg3:sem:tag-trie.cg3.trie-serialize-fn]
> Serializes the trie to a big-endian byte stream. It does NOT write the
> top-level entry count — the caller emits that before invoking this. For
> each kv in ascending Tag-hash order (flat_map order): writeBE<uint32_t>(
> kv.first->number) writes the Tag's `number` field as 4 big-endian bytes;
> writeBE<uint8_t>(kv.second.terminal) writes 1 byte (0 or 1). Then, if
> kv.second.trie is non-null: writeBE<uint32_t>(UI32(sub->size())) writes
> the child count as 4 big-endian bytes, followed by a recursive
> trie_serialize(*sub, out); otherwise writeBE<uint32_t>(0) writes a zero
> child count. Per-node byte layout: [number: u32 BE][terminal: u8]
> [childCount: u32 BE][serialized children...]. BYTE-PARITY NOTES: the
> emitted node identifier is Tag->number while the iteration/order key is
> Tag->hash (they need not correlate); writeBE serializes via boost
> native_to_big, i.e. most-significant byte first.

> [spec:cg3:def:tag-trie.cg3.trie-singular-fn]
> inline bool trie_singular(const trie_t& trie)

> [spec:cg3:sem:tag-trie.cg3.trie-singular-fn]
> Returns true iff the trie is a single non-branching chain that ends in a
> terminal. If trie.size() != 1, return false. Otherwise let node =
> trie.begin()->second (the sole entry's node). If node.terminal is true,
> return true. Else if node.trie is non-null, return trie_singular(*node.
> trie) (recurse into the only child level). Else return false (a lone
> non-terminal entry with no sub-trie). Net: true exactly when every level
> has exactly one key and the chain reaches a terminal.

> [spec:cg3:def:tag-trie.cg3.trie-t]
> typedef bc::flat_map<Tag*, trie_node_t, compare_Tag> trie_t

