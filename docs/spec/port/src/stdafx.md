# src/stdafx.hpp

> [spec:cg3:def:stdafx.cg3.flags-t]
> struct flags_t {
>   uint64_t flags = 0;
>   int32_t sub_reading = 0;
> }

> [spec:cg3:def:stdafx.cg3.u-string]
> typedef std::basic_string<UChar> UString

> [spec:cg3:def:stdafx.cg3.u-string-vector]
> typedef std::vector<UString> UStringVector

> [spec:cg3:def:stdafx.cg3.u-string-view]
> typedef std::basic_string_view<UChar> UStringView

> [spec:cg3:def:stdafx.cg3.uint32-vector]
> typedef std::vector<uint32_t> uint32Vector

