// S314 — BigInt.{asIntN,asUintN}(bits, value, ...trailing) per
// ES §21.2.2.{1,2}: spec sig is 2-arg (bits + value); trailing
// args MUST be silently ignored. Pre-S314 check.rs's fixed
// (Type::Object("BigInt"), m) method-table sig rejected args.len()
// >= 3 with "expected 2 argument(s), got M". The narrow widen arm
// (placed beside S309 Object.fromEntries) typecheck-drops args[2..];
// ssa_lower's bigint asIntN/asUintN gate widens `== 2` → `>= 2`
// and adds skip(2) lower-and-drop loop (S272 idiom).

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

// 1 trailing
const r1 = BigInt.asIntN(8, 256n, step("t1"));
console.log("r1:", r1, "calls:", calls);

// 2 trailing
const r2 = BigInt.asIntN(8, 256n, step("t2a"), step("t2b"));
console.log("r2:", r2, "calls:", calls);

// 3 trailing
const r3 = BigInt.asIntN(8, 256n, step("t3a"), step("t3b"), step("t3c"));
console.log("r3:", r3, "calls:", calls);

// asUintN: 1 trailing
const r4 = BigInt.asUintN(8, 256n, step("t4"));
console.log("r4:", r4, "calls:", calls);

// asUintN: 2 trailing
const r5 = BigInt.asUintN(8, 256n, step("t5a"), step("t5b"));
console.log("r5:", r5, "calls:", calls);

// asUintN: 3 trailing
const r6 = BigInt.asUintN(8, 256n, step("t6a"), step("t6b"), step("t6c"));
console.log("r6:", r6, "calls:", calls);
