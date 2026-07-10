# src/bloomish.hpp

> [spec:cg3:def:bloomish.cg3.bloomish]
> class bloomish {
>   Cont value[4];
> }

> [spec:cg3:def:bloomish.cg3.bloomish.bloomish-fn]
> bloomish()

> [spec:cg3:sem:bloomish.cg3.bloomish.bloomish-fn]
> Default constructor. Immediately calls clear(), which zero-fills all
> four Cont slots value[0..3]. The result is an empty filter whose every
> bucket is 0. No other state exists.

> [spec:cg3:def:bloomish.cg3.bloomish.clear-fn]
> void clear()

> [spec:cg3:sem:bloomish.cg3.bloomish.clear-fn]
> Sets all four elements value[0], value[1], value[2], value[3] to
> static_cast<Cont>(0) via std::fill over the range [value, value+4).
> Returns nothing. This is the reset used by the constructor.

> [spec:cg3:def:bloomish.cg3.bloomish.insert-fn]
> void insert(const Cont& v)

> [spec:cg3:sem:bloomish.cg3.bloomish.insert-fn]
> Selects exactly one of the four buckets by testing the low bits of v in
> strict priority order and OR-accumulates the whole value v into it:
> if (v & 4) value[3] |= v; else if (v & 2) value[2] |= v;
> else if (v & 1) value[1] |= v; else value[0] |= v. That is, bit 2
> (mask 4) wins over bit 1 (mask 2) over bit 0 (mask 1); if none of bits
> 0-2 are set, bucket 0 is used. The full v (all its bits, not just the
> selector bit) is OR'd into the chosen bucket; nothing else changes.
> insert(0) targets bucket 0 and is a no-op.

> [spec:cg3:def:bloomish.cg3.bloomish.matches-fn]
> bool matches(const Cont& v) const

> [spec:cg3:sem:bloomish.cg3.bloomish.matches-fn]
> Membership test that can yield false positives but never false
> negatives. Chooses a bucket with the exact same priority logic as
> insert (bit 2 set -> bucket 3, else bit 1 -> bucket 2, else bit 0 ->
> bucket 1, else bucket 0) and returns (value[bucket] & v) == v — i.e.
> true iff every set bit of v is present in that bucket's accumulated OR.
> Because insert only OR-accumulates, a value that was never inserted can
> still match when the union of previously inserted values covers all of
> v's bits. matches(0) is always true (bucket 0, (x & 0) == 0).

> [spec:cg3:def:bloomish.cg3.uint32-bloomish]
> typedef bloomish<uint32_t> uint32Bloomish

