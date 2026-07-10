// chunk 753 — dynamic index reads on a typed struct receiver ride
// the any-index runtime lane (ES ToPropertyKey): numeric loop vars,
// string variable keys, for-of keys, miss -> undefined, OptIndex,
// const-folded indices. The NaN-box receiver encoding is a borrow —
// repeated reads must not release the struct cell (interleaved
// allocs after loops previously read a freed cell).
const g = { 0: "zero", 1: "one", 2: "two" };
for (let i = 0; i < 4; i++) console.log(g[i]);
const s = { a: 1, b: "x", c: true };
const keys = ["a", "b", "c", "zzz"];
for (const k of keys) console.log(s[k]);
let acc = 0;
const nums = { 0: 10, 1: 20 };
for (let i = 0; i < 2; i++) acc += nums[i];
console.log(acc);
const dyn = "b";
console.log(s[dyn]);
console.log(g?.[1]);
const idx = 1 + 1;
console.log(g[idx]);
let j = 1;
console.log(g[j], g[j], g[j]);
