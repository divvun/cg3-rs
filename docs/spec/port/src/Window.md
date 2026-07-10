# src/Window.cpp, src/Window.hpp

> [spec:cg3:def:window.cg3.single-window-cont]
> typedef std::vector<SingleWindow*> SingleWindowCont

> [spec:cg3:def:window.cg3.window]
> class Window {
>   GrammarApplicator* parent = nullptr;
>   uint32_t cohort_counter = 0;
>   uint32_t window_counter = 0;
>   uint32_t window_span = 0;
>   std::map<uint32_t, Cohort*> cohort_map;
>   uint32FlatHashMap dep_map;
>   std::map<uint32_t, Cohort*> dep_window;
>   uint32FlatHashMap relation_map;
>   SingleWindowCont previous;
>   SingleWindow* current = nullptr;
>   SingleWindowCont next;
> }

> [spec:cg3:def:window.cg3.window.alloc-append-single-window-fn]
> SingleWindow* Window::allocAppendSingleWindow()

> [spec:cg3:sem:window.cg3.window.alloc-append-single-window-fn]
> Allocates a new `SingleWindow` via `alloc_swindow(this)`, does `++window_counter`,
> and assigns the new counter value to `swindow->number`. Links it to the BACK of
> the pending stream: if `next` is non-empty, set `swindow->previous = next.back()`
> and `next.back()->next = swindow`. Then `next.push_back(swindow)`. Returns the
> pointer. Quirk: if `next` is empty, NO sibling links are set at all — in
> particular it is not linked to `current`, so its `previous` stays null (unlike
> `allocPushSingleWindow`).

> [spec:cg3:def:window.cg3.window.alloc-push-single-window-fn]
> SingleWindow* Window::allocPushSingleWindow()

> [spec:cg3:sem:window.cg3.window.alloc-push-single-window-fn]
> Allocates a new `SingleWindow` via `alloc_swindow(this)`, does `++window_counter`,
> and assigns the counter to `swindow->number`. Links it to the FRONT of the
> pending stream: if `next` is non-empty, set `swindow->next = next.front()` and
> `next.front()->previous = swindow`; if `current` is set, set
> `swindow->previous = current` and `current->next = swindow`. Inserts `swindow`
> at the beginning of `next` (`next.insert(next.begin(), swindow)`). Returns the
> pointer. Effect: the new window becomes the frontmost pending one (the next to
> be shuffled in).

> [spec:cg3:def:window.cg3.window.alloc-single-window-fn]
> SingleWindow* Window::allocSingleWindow()

> [spec:cg3:sem:window.cg3.window.alloc-single-window-fn]
> Allocates a fresh `SingleWindow` via `alloc_swindow(this)`, does
> `++window_counter`, assigns the new counter value to `swindow->number`, and
> returns the pointer. Does NOT insert it into `previous`/`current`/`next` nor set
> any sibling links — a bare allocation with only its number set.

> [spec:cg3:def:window.cg3.window.back-fn]
> SingleWindow* Window::back()

> [spec:cg3:sem:window.cg3.window.back-fn]
> Returns the last single-window of the document: `next.back()` if `next` is
> non-empty; else `current` if it is set; else `previous.back()` if `previous` is
> non-empty; else `nullptr`.

> [spec:cg3:def:window.cg3.window.rebuild-cohort-links-fn]
> void Window::rebuildCohortLinks()

> [spec:cg3:sem:window.cg3.window.rebuild-cohort-links-fn]
> Rebuilds the global cohort `prev`/`next` chain across the whole document. Picks
> the first single-window: `previous.front()` if `previous` is non-empty, else
> `current`, else `next.front()` (else stays null). Then walks single-windows via
> their `->next` links; within each, iterates its `cohorts` in order keeping a
> running `prev` cohort (starting null): set `citer->prev = prev`,
> `citer->next = nullptr`, and if `prev` is non-null set `prev->next = citer`; then
> `prev = citer`. Continues across window boundaries, so the cohort links span the
> entire document. Relies on the single-window `->next` chain already being
> correct (see `rebuildSingleWindowLinks`). No return value.

> [spec:cg3:def:window.cg3.window.rebuild-single-window-links-fn]
> void Window::rebuildSingleWindowLinks()

> [spec:cg3:sem:window.cg3.window.rebuild-single-window-links-fn]
> Rebuilds the `previous`/`next` sibling pointer chain across all single-windows in
> document order. Keeps a running `sWindow` (the previous node, starting null) and
> visits, in sequence: every element of `previous`, then `current` (if set), then
> every element of `next`. For each visited window: set its `previous = sWindow`,
> and if `sWindow` is non-null set `sWindow->next = <this window>`; then advance
> `sWindow` to this window. After the walk, if any window was visited, set the last
> one's `next = nullptr`. Produces a consistent doubly-linked chain from head to
> tail. No return value.

> [spec:cg3:def:window.cg3.window.shuffle-windows-down-fn]
> void Window::shuffleWindowsDown()

> [spec:cg3:sem:window.cg3.window.shuffle-windows-down-fn]
> Advances the active window. If `current` is set: snapshot the applicator's
> current variable state into `current->variables_set = parent->variables`, clear
> `current->variables_rem`, push `current` onto the back of `previous`, then set
> `current = nullptr`. After that, if `next` is non-empty, pop its front into
> `current` (`current = next.front(); next.erase(next.begin())`). Net effect: the
> old current moves into history (carrying its variable snapshot) and the frontmost
> pending window becomes current; if `next` was empty, `current` ends up null. No
> return value.

> [spec:cg3:def:window.cg3.window.window-fn]
> Window::~Window()

> [spec:cg3:sem:window.cg3.window.window-fn]
> Destructor. For each `SingleWindow*` in `previous`, call `free_swindow(iter)`
> (which `clear()`s it and returns it to the shared thread-local pool). Then
> `free_swindow(current)`. Then for each in `next`, `free_swindow`. Recycles all of
> the window's single-windows; the `previous`/`next` vectors and other members are
> destroyed afterward by their own destructors. (The loops iterate by value, so the
> pointers inside the vectors are not themselves nulled, but the vectors die
> immediately after.)

