// S309 — Object.fromEntries(entries, ...trailing) per ES §20.1.2.7:
// spec sig is 1-arg (entries iterable); trailing args MUST be
// silently ignored. Pre-S309 the fixed (Type::Object("Object"),
// "fromEntries") method-table sig rejected `args.len() >= 2` with
// "expected 1 argument(s), got M". check.rs S309 narrow widen
// typecheck-drops args[1..]; ssa_lower mirror widens
// is_fromentries_call gate + LetDecl fast-path lowers trailing for
// side-effects then drops. step()-counter verifies bun-parity on
// observable evaluation order (trailing args MUST evaluate left-to-
// right, count == trailing.len()).

let calls = 0;
const step = (x: any) => {
  calls = calls + 1;
  return x;
};

type Pair = { a: number; b: number };

// 1 trailing arg
const entries1: Array<Array<any>> = [
  ["a", 1],
  ["b", 2],
];
const o1: Pair = Object.fromEntries(entries1, step("t1"));
console.log("o1.a:", o1.a, "o1.b:", o1.b, "calls:", calls);

// 2 trailing args
const entries2: Array<Array<any>> = [
  ["a", 10],
  ["b", 20],
];
const o2: Pair = Object.fromEntries(entries2, step("t2a"), step("t2b"));
console.log("o2.a:", o2.a, "o2.b:", o2.b, "calls:", calls);

// 3 trailing args
const entries3: Array<Array<any>> = [
  ["a", 100],
  ["b", 200],
];
const o3: Pair = Object.fromEntries(
  entries3,
  step("t3a"),
  step("t3b"),
  step("t3c"),
);
console.log("o3.a:", o3.a, "o3.b:", o3.b, "calls:", calls);
