// W-O-1 — Object.values(arr) returns a fresh shallow array copy
// Spec ES §20.1.2.20 step 2: ToObject(Arr) + own-keys walk on an
// Array exotic = the slot values in numeric order. tora reuses the
// production deep-clone pattern (arr_slice + per-element rc_inc for
// refcounted elem types) shared with typed-struct Arr-field copy.
// Pre-fix tora rejected at check.rs "Object.values requires a
// struct arg, got Array(Number)".
//
// 4 shapes via named typed bindings: multi-elem / empty / single /
// Arr<Str> (refcounted elem rc_inc path). Inline empty literal []
// infers Arr<Any> (16-byte stride) which doesn't yet have an
// arr_slice_any helper — L3b W-O-1-empty-any.

const a: number[] = [10, 20, 30];
console.log(Object.values(a));

const b: number[] = [];
console.log(Object.values(b));

const c: number[] = [42];
console.log(Object.values(c));

const d: string[] = ["a", "b", "c"];
console.log(Object.values(d));
