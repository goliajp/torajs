// Multi-arg `console.log` inspect-format substrate close-mark.
//
// Pre-substrate: ssa-lower's multi-arg `console.log` joiner coerced
// every arg to a `Str` via `coerce_to_str` + `str_concat` chain,
// then printed the joined string. typed Arr / Obj / Map / Set /
// etc. fell into the `panic!("ssa-lower: console multi-arg coercion
// of type {other:?} not supported")` arm so `console.log("label",
// arr)` was unreachable for any typed receiver.
//
// New (this trunk): per-arg inspect dispatch in the lowerer:
//
// - typed `Array<i64|f64|bool|str|substr>` → matching
//   `__torajs_arr_print_<T>_inline` from `torajs-arr::print_inline`
//   (no-\n typed walker family, fork of the trailing-\n family used
//   by the single-arg path).
// - Everything else (Str / Substr / I64 / F64 / Bool / Type::Any
//   pass-through / typed Obj / Map / Set / Date / RegExp / Promise
//   / Closure / Symbol / BigInt / Weak* / etc.) → box-to-Any (or
//   pass through if already Type::Any) +
//   `__torajs_print_anyv_inline_top` (a no-\n top-level variant of
//   `__torajs_print_anyv` — Str unquoted, matches bun's
//   `console.log("hi", 1)` → `hi 1`).
//
// Between args the lowerer emits a single ' ' via
// `__torajs_io_putc_stdout`, and after the last arg a single '\n'.
// `console.error` / `console.warn` keep the pre-existing Str-coerce
// path (less coverage in conformance, not part of this trunk).
//
// This fixture exercises all the per-type arms one shape per line
// and asserts byte-equality vs bun.

// 1. Two-arg Str-Str (regression — pre-substrate worked).
console.log("hello", "world");

// 2. Str + typed Array<i64> — the canonical wedge the pre-substrate
//    `coerce_to_str` panicked on.
console.log("nums", [1, 2, 3]);

// 3. Str + typed Array<F64>.
console.log("floats", [1.5, 2.5, 3.5]);

// 4. Str + typed Array<Bool>.
console.log("flags", [true, false, true]);

// 5. Str + typed Array<Str> — nested string elements bun-form
//    quotes per the typed walker.
console.log("strs", ["a", "b", "c"]);

// 6. Str + typed Obj instance — class with declared fields.
class P {
    x: number = 0;
    y: number = 0;
    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }
}
let p = new P(1, 2);
console.log("typed obj", p);

// 7. Str + DynObj literal.
let h = { a: 1, b: "two" };
console.log("hetero", h);

// 8. Two typed Arr args.
let arr1 = [1, 2, 3];
let arr2 = [4, 5];
console.log("arrs", arr1, arr2);

// 9. Str + I64 + Bool + Str — primitive scalar mix.
console.log("vals", 42, true, "str");

// 10. Str + Map.
let m = new Map<string, number>();
m.set("a", 1);
console.log("map", m);

// 11. Str + Set.
let s = new Set<number>();
s.add(7);
console.log("set", s);

// 12. Str + Date.
let d = new Date(0);
console.log("date", d);

// 13. Three-arg label / value / note.
console.log("idx", 0, "value");

// 14. Single-arg regression — typed Arr / Obj / Map single-arg
//     path stays on the trailing-\n typed walker family (not the
//     no-\n inline variants).
console.log(42);
console.log("solo");
console.log([1, 2]);
console.log(p);
