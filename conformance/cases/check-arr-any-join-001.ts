// Array<Any>.join(sep) — W-J trunk-close follow-up (plan-state line
// 1550 "新揭 L3b ①"). Pre-fix `check.rs` rejected:
//   "no member `.join` on type Array(Any)"
// because the runtime helper dispatch only had Str / Substr / I64 /
// F64 / Bool variants — Array<Any> slots are NaN-box AnyValue u64s
// (Step 7e-A: 8-byte stride, same as typed-tier) and using the Str
// variant on them dereferences the high NaN-box bits as a Str ptr.
//
// New `__torajs_arr_join_any` walks the slots, delegates ToString to
// `__torajs_anyv_to_str`, and special-cases undefined / null →
// empty string per spec §22.1.3.15.5 (Array.join overrides the
// default ToString for the two sentinels).

const xs: any[] = ["alpha", 1, true, "beta"];
console.log(xs.join(","));
console.log(xs.join(" | "));

// Spec §22.1.3.15.5: undefined / null → empty string.
const ys: any[] = ["a", undefined, "b", null, "c"];
console.log(ys.join(","));
console.log(ys.join("-"));

// Empty array → empty string. Single element → element ToString
// (no separator emitted).
const empty: any[] = [];
console.log(empty.join(","));
console.log("[" + empty.join(",") + "]");

const one: any[] = [42];
console.log(one.join(","));

// Default sep = "," when no arg given (V3-18 m1.h.42).
const zs: any[] = [1, 2, 3];
console.log(zs.join());

// `arr.toString()` is `arr.join(",")` per spec §22.1.3.30.
console.log(xs.toString());

// W-J trunk-close synergy: `Object.keys(struct)` returns Array<Str>
// (struct-cell path, not Array<Any>), so this fixture covers the
// independent Array<Any>.join axis. The struct-via-Any keys path is
// a separate `non-struct-via-any reflection` trunk (plan-state line
// 1550 W-J narrow-surface limit).
