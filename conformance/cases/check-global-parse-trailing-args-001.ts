// S313 — global parseInt(str, radix, ...trailing) + global
// parseFloat(str, ...trailing) per ES §19.2.5 / §19.2.4 trailing-
// arg ignore. check.rs S252 already typecheck-drops trailing
// (5869 / 5909 skip loops); but ssa_lower's bare-Ident global
// path (ssa_lower.rs:17219 / 17264) had no lower-and-drop mirror
// — return value byte-equal (parseInt/parseFloat return correct
// Number) but step()-style trailing args were silently NOT
// lowered, so their side-effects were lost.
//
// classic silent-drop pattern: check.rs typecheck-and-drop ≠
// ssa lower-and-drop. Sister fix to S302 (Number.parseInt/
// parseFloat namespace path). probe-first revealed via tora
// calls=0 vs bun=4 with identical return values.

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

// global parseInt: 1 trailing
const r1 = parseInt("42", 10, step("t1"));
console.log("r1:", r1, "calls:", calls);

// global parseInt: 2 trailing
const r2 = parseInt("0xff", 16, step("t2a"), step("t2b"));
console.log("r2:", r2, "calls:", calls);

// global parseInt: 3 trailing
const r3 = parseInt("100", 10, step("t3a"), step("t3b"), step("t3c"));
console.log("r3:", r3, "calls:", calls);

// global parseFloat: 1 trailing
const r4 = parseFloat("3.14", step("t4"));
console.log("r4:", r4, "calls:", calls);

// global parseFloat: 2 trailing
const r5 = parseFloat("2.718", step("t5a"), step("t5b"));
console.log("r5:", r5, "calls:", calls);

// global parseFloat: 3 trailing
const r6 = parseFloat("0.5", step("t6a"), step("t6b"), step("t6c"));
console.log("r6:", r6, "calls:", calls);
