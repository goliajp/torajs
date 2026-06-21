// S316 — Map.forEach(cb, ...trailing) per ES §23.1.3.5 + Set.forEach(
// cb, ...trailing) per ES §24.2.3.6: trailing args MUST be evaluated
// left-to-right (their side-effects fire) but their values are
// silently ignored (the second arg is normally thisArg which tora
// doesn't bind anyway). check.rs S270 (8588) widened the typecheck
// to accept trailing args, BUT ssa_lower's Map.forEach (21816) and
// Set.forEach (21229) had `debug_assert_eq!(args.len(), 1)` strict
// gate — release build skipped the assert and lowered only args[0]
// (cb), silently dropping trailing args' side-effects.
//
// Classic silent-drop pattern revealed by step()-counter (return
// value undefined in both bun & tora is byte-equal; only calls=0
// vs bun=4 reveals the missing side-effects).

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);

let msum = 0;
m.forEach((v: number, k: string) => {
  msum = msum + v;
}, step("t1"));
console.log("msum after t1:", msum, "calls:", calls);

m.forEach((v: number) => {
  msum = msum + v;
}, step("t2a"), step("t2b"));
console.log("msum after t2:", msum, "calls:", calls);

const s = new Set<number>();
s.add(10);
s.add(20);

let ssum = 0;
s.forEach((v: number) => {
  ssum = ssum + v;
}, step("t3"));
console.log("ssum after t3:", ssum, "calls:", calls);

s.forEach((v: number) => {
  ssum = ssum + v;
}, step("t4a"), step("t4b"), step("t4c"));
console.log("ssum after t4:", ssum, "calls:", calls);
