// S311 — JSON.stringify(value, replacer, space) per ES §25.5.2:
// replacer/space are evaluated left-to-right but tora's stringify
// substrate currently ignores them (replacer/space substrate L3b).
// Pre-S311 check.rs:5772 widened to accept 1..=3 args (typecheck-
// and-drop), but ssa_lower's dispatch entry (line 16993, the
// universal path before the M6.3 primitive-only fallback) only
// lowered args[0] then returned, silently dropping replacer +
// space at lower-time. step()-counter fixture verifies bun-parity
// on left-to-right side-effect evaluation (return value byte-equal
// hid this silent-drop; calls=0 vs bun=6 revealed it).

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

// primitive value + 1 trailing (replacer)
const r1 = JSON.stringify(42, step("t1"));
console.log("r1:", r1, "calls:", calls);

// primitive value + 2 trailing (replacer + space)
const r2 = JSON.stringify("abc", step("t2a"), step("t2b"));
console.log("r2:", r2, "calls:", calls);

// boolean + 2 trailing
const r3 = JSON.stringify(true, step("t3a"), step("t3b"));
console.log("r3:", r3, "calls:", calls);

// number with f64 fraction + 2 trailing
const r4 = JSON.stringify(3.14, step("t4a"), step("t4b"));
console.log("r4:", r4, "calls:", calls);

// primitive (no space-indent effect) + 3 trailing
const r5 = JSON.stringify(0, step("t5a"), step("t5b"), step("t5c"));
console.log("r5:", r5, "calls:", calls);
