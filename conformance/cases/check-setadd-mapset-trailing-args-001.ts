// S312 — Set.add(value, ...trailing) per ES §24.2.3.1 + Map.set(
// key, value, ...trailing) per ES §23.1.3.9: trailing args MUST
// be evaluated left-to-right (their side-effects fire) then the
// value is silently ignored. check.rs S248 widened the typecheck
// to accept the trailing shape, but ssa_lower's emit at
// ssa_lower.rs:20929 (Set.add) / :21490 (Map.set) skipped the
// lower-and-drop loop — comments claimed "args[1..] silent ignored
// at lower-time" but actually the side-effects were dropped at
// lower-time too (silent-wrong, not silent-drop-of-result).
//
// This was the silent-wrong pattern that "trailing OK" prior
// handoff missed: return value byte-equal (Set.add returns the
// Set; Map.set returns the Map) AND the typecheck passes, so
// only step()-counter probe revealed the missing side-effect.

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

const s = new Set<number>();
s.add(1, step("t1"));
console.log("after add(1,t1) calls:", calls);

s.add(2, step("t2a"), step("t2b"));
console.log("after add(2,t2a,t2b) calls:", calls);

s.add(3, step("t3a"), step("t3b"), step("t3c"));
console.log("after add(3,...3) calls:", calls);

const m = new Map<string, number>();
m.set("a", 1, step("t4"));
console.log("after set(a,1,t4) calls:", calls);

m.set("b", 2, step("t5a"), step("t5b"));
console.log("after set(b,2,t5a,t5b) calls:", calls);

m.set("c", 3, step("t6a"), step("t6b"), step("t6c"));
console.log("after set(c,3,...3) calls:", calls);

console.log("set has 1:", s.has(1), "has 4:", s.has(4));
console.log("map get a:", m.get("a"), "get b:", m.get("b"), "get c:", m.get("c"));
