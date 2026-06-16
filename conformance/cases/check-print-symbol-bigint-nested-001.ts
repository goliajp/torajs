// S130-3 narrow: nested-context + Any-typed Symbol / BigInt print.
// Pre-fix the Tag::Symbol / Tag::BigInt arms were missing from both
// inspect dispatchers (`any.rs` top-level cell branch +
// `tag_dispatch.rs` nested-inline), so `[sym, big]` / `{ s: sym, b: big }`
// / `console.log(symAsAny)` / `console.log(bigAsAny)` all fell through
// to the generic `[object]` placeholder instead of `Symbol(<desc>)` /
// `<n>n`. Type-specialized top-level (sym typed Symbol / big typed
// BigInt) already worked via the IR-emitted print path; this commit
// only patches the Any cell-tag dispatchers.
const sym: any = Symbol("hi");
const big: any = 99n;

// Top-level Any cell — exercises any.rs cell branch.
console.log(sym);
console.log(big);

// Nested via Array<Any> — exercises tag_dispatch inline path.
console.log([sym, big]);

// Nested via dynobj `{}` — same inline dispatcher (obj_print_any
// calls __torajs_print_anyv_inline per slot).
console.log({ s: sym, b: big });

// Mixed with other Any-typed primitives — sanity that the new arms
// don't disturb existing Str / Bool / Number routing.
console.log(["a", 1, sym, big, true]);
