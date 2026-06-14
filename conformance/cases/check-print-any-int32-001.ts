// single-arg console.log(any: int32) — `print_anyv` int32 arm
// (inspect.rs:156-160) → `print_i64`.  Pairs with check-print-any-
// shortstr-001 for the Any-typed primitive baseline.  Regression guard
// for ⑤-c probe finding: probe-4 was the lone Any-typed Number primitive
// whose tr output matched bun; the int32-NaN-box → `as_int32` → i64 →
// `print_i64` walk has no direct fixture (console-001 path B reaches
// the same arm via a multi-arg coerce path, not the single-arg
// dispatch).
const n: any = 42;
console.log(n);
