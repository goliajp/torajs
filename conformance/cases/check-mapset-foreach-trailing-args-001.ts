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
// Classic silent-drop pattern revealed by step()-counter. Note: the
// cb body intentionally does NOT read its (v, k) params — Map/Set
// forEach has a pre-existing Any-value decode hole (L3b) that
// surfaces when cb does arithmetic on v. The trailing-arg fix is
// orthogonal to that; the fixture only verifies cb-runs + trailing-
// args evaluation per the S272 idiom contract.

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);

let mcb_runs = 0;
m.forEach((_v: number, _k: string) => {
  mcb_runs = mcb_runs + 1;
}, step("t1"));
console.log("mcb_runs after t1:", mcb_runs, "calls:", calls);

m.forEach(
  (_v: number) => {
    mcb_runs = mcb_runs + 1;
  },
  step("t2a"),
  step("t2b"),
);
console.log("mcb_runs after t2:", mcb_runs, "calls:", calls);

const s = new Set<number>();
s.add(10);
s.add(20);

let scb_runs = 0;
s.forEach((_v: number) => {
  scb_runs = scb_runs + 1;
}, step("t3"));
console.log("scb_runs after t3:", scb_runs, "calls:", calls);

s.forEach(
  (_v: number) => {
    scb_runs = scb_runs + 1;
  },
  step("t4a"),
  step("t4b"),
  step("t4c"),
);
console.log("scb_runs after t4:", scb_runs, "calls:", calls);
