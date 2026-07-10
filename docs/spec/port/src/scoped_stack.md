# src/scoped_stack.hpp

> [spec:cg3:def:scoped-stack.cg3.scoped-stack]
> struct scoped_stack {
>   struct proxy { proxy(scoped_stack* ss) : z(ss->z++) , ss(ss) { if (ss->cs.size() < ss->z) { ss->cs.resize(ss->z); } } ~proxy() { ss->cs[z].clear(); --ss->z; ...;
>   size_t z;
>   std::vector<C> cs;
> }

> [spec:cg3:def:scoped-stack.cg3.scoped-stack.get-fn]
> proxy get()

> [spec:cg3:sem:scoped-stack.cg3.scoped-stack.get-fn]
> Returns a new `proxy` bound to this stack (`return proxy(this)`). The
> proxy reserves the next slot: it takes the current depth `z` as its
> index, post-increments `ss->z`, and grows the backing vector `cs` to hold
> the new depth if needed (see the proxy constructor sem). While the
> returned proxy is alive it gives access (via `operator->`, `operator*`,
> `operator C&`) to a `C` object at that slot; when the proxy is destroyed
> that `C` is cleared and the depth is decremented. Intended use is a
> RAII-scoped temporary: `auto p = ss.get();` then `p->...`. Because slots
> are reused across scopes, the borrowed `C` may retain capacity from a
> previous user (it is `clear()`-ed on release, not on acquire).

> [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy]
> struct proxy {
>   size_t z;
>   scoped_stack* ss;
> }

> [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.operator-fn]
> C* operator->()

> [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.operator-fn]
> Arrow-dereference operator: returns `&ss->cs[z]`, a pointer to this
> proxy's `C` object — the element at this proxy's reserved index `z` in
> the owning stack's backing vector. Enables `proxy->member` access to the
> scoped temporary. (Sibling `operator*` returns a reference `ss->cs[z]`,
> and the implicit `operator C&` conversion also yields that reference.)

> [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
> proxy(scoped_stack* ss)

> [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
> Constructs a proxy that reserves the next slot on the owning stack `ss`.
> Member `z` is initialized to the current `ss->z` and then `ss->z` is
> post-incremented (so `z` is the pre-increment depth = this proxy's slot
> index, and the stack's depth grows by one). Stores `ss`. If the backing
> vector is smaller than the new depth (`ss->cs.size() < ss->z`), resize it
> to `ss->z`, default-constructing new `C` objects as needed. The vector
> only ever grows here (never shrinks), so `C` objects are pooled and
> reused across nested scopes. The paired destructor (`~proxy`) does
> `ss->cs[z].clear()` then `--ss->z`, releasing the slot in LIFO order.
> Assumes proxies are created and destroyed in strict stack order.

> [spec:cg3:def:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
> scoped_stack()

> [spec:cg3:sem:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
> Default constructor. Initializes the current depth `z` to 0; the backing
> vector `cs` is default-constructed empty. No `C` slots are allocated
> yet — they are created lazily by proxies (via `get`) as the stack grows.

