# src/stdafx.hpp

> [spec:cg3:def:stdafx.cg3.flags-t]
> struct flags_t {
>   uint64_t flags = 0;
>   int32_t sub_reading = 0;
> }

`[spec:cg3:def:stdafx.cg3.u-string]`, `[spec:cg3:def:stdafx.cg3.u-string-vector]`
and `[spec:cg3:def:stdafx.cg3.u-string-view]` stood here, naming the C++
`typedef std::basic_string<UChar> UString`,
`typedef std::vector<UString> UStringVector` and
`typedef std::basic_string_view<UChar> UStringView`. They are obsolesced, not
unmet: each names a UTF-16 text type — `UChar` is ICU's 16-bit code unit, and
the three typedefs exist so the C++ can spell "a string of code units" without
repeating `basic_string<UChar>`. This port has no ICU and no UTF-16
representation; its text is `String` / `Vec<String>` / `&str`, UTF-8 end to
end. Aliasing those std types back to the C++ spellings was a rename, not a
representation, so there is no port symbol left for the ids to name. The
code-unit-vs-scalar consequences the typedefs imply are documented where they
bite — the char-offset span contract in `ast.rs`, the `&[char]` scanning
convention in `inlines`, and the transcoding-collapses-to-identity note in
`uextras.rs`.

> [spec:cg3:def:stdafx.cg3.uint32-vector]
> typedef std::vector<uint32_t> uint32Vector

