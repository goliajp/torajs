// single-arg console.log(any: short-str) — `print_anyv` ShortStr inline
// path (inspect.rs:174-183).  Pairs with console-001 (multi-arg catch-
// bound Any) — this case nails the dominant `const s: any = "hi"`
// surface that goes through `__torajs_print_anyv` single-arg dispatch
// with the immediate ShortStr-tagged NaN-box (no heap alloc, putchar
// per byte + trailing newline).  Regression guard for ⑤-c probe
// finding: probe-3 was the lone Any-typed primitive whose tr output
// already matched bun, but `print_anyv` short_str_bytes / putchar walk
// has no test coverage today (console-001 path C/D hit bool/null arms
// directly).
const s: any = "hi";
console.log(s);
