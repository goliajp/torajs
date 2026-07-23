// `Object.groupBy(items, callbackFn)` per ES §20.1.2.10 — Array
// items lane (iterable-only receivers are L3b — spec step 2 requires
// the full IteratorRecord walk). Kernel walks the array and
// dispatches cb via the uniform any-call ABI.

// Basic: partition by kind, values are grouped in insertion order.
const items = [
  { kind: "a", n: 1 },
  { kind: "b", n: 2 },
  { kind: "a", n: 3 },
];
const g1 = Object.groupBy(items as any, (x: any) => x.kind);
console.log(JSON.stringify(g1));
// {"a":[{"kind":"a","n":1},{"kind":"a","n":3}],"b":[{"kind":"b","n":2}]}

// Even / odd on numbers — key is arbitrary property-key string.
const nums = [1, 2, 3, 4, 5, 6];
const g2 = Object.groupBy(nums as any, (n: any) => (n % 2 === 0 ? "even" : "odd"));
console.log(JSON.stringify(g2));
// {"odd":[1,3,5],"even":[2,4,6]}

// Callback receives (item, index) — index-based key.
const letters = ["a", "b", "c", "d"];
const g3 = Object.groupBy(letters as any, (_: any, i: any) => (i < 2 ? "first" : "rest"));
console.log(JSON.stringify(g3));
// {"first":["a","b"],"rest":["c","d"]}

// Non-string keys coerce via ToPropertyKey (number → decimal string).
const nums2 = [0.1, 0.2, 1.7, 2.9];
const g4 = Object.groupBy(nums2 as any, (n: any) => Math.floor(n));
console.log(JSON.stringify(g4));
// {"0":[0.1,0.2],"1":[1.7],"2":[2.9]}

// Empty array → empty null-prototype object.
const g5 = Object.groupBy([] as any, (_: any) => "x");
console.log(JSON.stringify(g5));
// {}

// Single-key partition: all items land in one bucket, order preserved.
const g6 = Object.groupBy([10, 20, 30] as any, (_: any) => "all");
console.log(JSON.stringify(g6));
// {"all":[10,20,30]}

// Boolean-returning key coerces to string "true" / "false".
const mixed = [1, 2, 3, 4];
const g7 = Object.groupBy(mixed as any, (n: any) => n > 2);
console.log(JSON.stringify(g7));
// {"false":[1,2],"true":[3,4]}
