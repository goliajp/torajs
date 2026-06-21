// S310 — Array<T>.{fill,copyWithin}(value/target, start, end, ...trailing)
// per ES §23.1.3.{4,7}: spec sig is 3-arg (value/target + start + end);
// trailing args silently ignored. Pre-S310 check.rs widen S246 capped at
// `args.len() == 4` (exactly 1 trailing); `>= 5` reject with "expected 3
// argument(s), got M". ssa_lower S298 skip(3) loop already drains
// args[3..] — only check.rs gate widen needed (`== 4` → `>= 4`).
// step()-counter verifies bun-parity on observable left-to-right
// evaluation order.

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

const a1 = [1, 2, 3, 4, 5];

// fill: 1 trailing
const r1 = a1.slice().fill(0, 1, 3, step("t1"));
console.log("r1:", r1, "calls:", calls);

// fill: 2 trailing
const r2 = a1.slice().fill(0, 1, 3, step("t2a"), step("t2b"));
console.log("r2:", r2, "calls:", calls);

// fill: 3 trailing
const r3 = a1.slice().fill(0, 1, 3, step("t3a"), step("t3b"), step("t3c"));
console.log("r3:", r3, "calls:", calls);

// copyWithin: 1 trailing
const r4 = a1.slice().copyWithin(0, 3, 5, step("t4"));
console.log("r4:", r4, "calls:", calls);

// copyWithin: 2 trailing
const r5 = a1.slice().copyWithin(0, 3, 5, step("t5a"), step("t5b"));
console.log("r5:", r5, "calls:", calls);

// copyWithin: 3 trailing
const r6 = a1
  .slice()
  .copyWithin(0, 3, 5, step("t6a"), step("t6b"), step("t6c"));
console.log("r6:", r6, "calls:", calls);
