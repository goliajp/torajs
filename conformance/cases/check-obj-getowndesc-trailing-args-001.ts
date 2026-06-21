// S315 — Object.getOwnPropertyDescriptor(obj, key, ...trailing)
// per ES §20.1.2.10: spec sig is 2-arg (obj + key); trailing args
// MUST be silently ignored. Pre-S315 check.rs's fixed sig rejected
// args.len() >= 3 with "expected 2 argument(s), got M". Narrow widen
// arm in check.rs typecheck-drops args[2..]; ssa_lower's dispatch
// gate at 18024 widens `== 2` → `>= 2` + adds skip(2) lower-and-
// drop loop after obj-lower (S272 idiom).

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

const a = { x: 1, y: 2 };

// 1 trailing
const r1 = Object.getOwnPropertyDescriptor(a, "x", step("t1"));
console.log("r1.value:", r1?.value, "calls:", calls);

// 2 trailing
const r2 = Object.getOwnPropertyDescriptor(a, "y", step("t2a"), step("t2b"));
console.log("r2.value:", r2?.value, "calls:", calls);

// 3 trailing
const r3 = Object.getOwnPropertyDescriptor(
  a,
  "x",
  step("t3a"),
  step("t3b"),
  step("t3c"),
);
console.log("r3.value:", r3?.value, "calls:", calls);

// Array.length fast path + trailing
const arr = [10, 20, 30];
const r4 = Object.getOwnPropertyDescriptor(arr, "length", step("t4a"), step("t4b"));
console.log("r4.value:", r4?.value, "calls:", calls);
