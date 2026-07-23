// `Map.groupBy(items, callbackFn)` per ES §24.2.2.4 — Array items
// lane (iterable-only receivers are L3b). Sister to
// Object.groupBy: keys are stored via SameValueZero (retain runtime
// type — no ToPropertyKey coercion) and the accumulator is a Map.

// Basic string keys — same-key hits push into shared bucket.
const items = [
  { kind: "a", n: 1 },
  { kind: "b", n: 2 },
  { kind: "a", n: 3 },
];
const g1 = Map.groupBy(items as any, (x: any) => x.kind);
console.log(g1.size);
console.log(JSON.stringify(g1.get("a")));
console.log(JSON.stringify(g1.get("b")));
// 2 / [{"kind":"a","n":1},{"kind":"a","n":3}] / [{"kind":"b","n":2}]

// Number keys — Map preserves the numeric type (Object.groupBy
// would coerce to string).
const nums = [1, 2, 3, 4, 5, 6];
const g2 = Map.groupBy(nums as any, (n: any) => n % 2);
console.log(g2.size);
console.log(JSON.stringify(g2.get(0)));
console.log(JSON.stringify(g2.get(1)));
// 2 / [2,4,6] / [1,3,5]

// Boolean keys — Map keeps `true` / `false` distinct from strings.
const mixed = [1, 2, 3, 4];
const g3 = Map.groupBy(mixed as any, (n: any) => n > 2);
console.log(g3.size);
console.log(JSON.stringify(g3.get(true)));
console.log(JSON.stringify(g3.get(false)));
// 2 / [3,4] / [1,2]

// Empty items → empty Map.
const g4 = Map.groupBy([] as any, (_: any) => "x");
console.log(g4.size);
// 0

// Callback receives (item, index).
const letters = ["a", "b", "c", "d"];
const g5 = Map.groupBy(letters as any, (_: any, i: any) => (i < 2 ? "first" : "rest"));
console.log(JSON.stringify(g5.get("first")));
console.log(JSON.stringify(g5.get("rest")));
// ["a","b"] / ["c","d"]

// All items into one bucket — order preserved.
const g6 = Map.groupBy([10, 20, 30] as any, (_: any) => "all");
console.log(g6.size);
console.log(JSON.stringify(g6.get("all")));
// 1 / [10,20,30]
