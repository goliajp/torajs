// S156 — `new Map(<typed Array<Array<T>>>)` iterable initializer per ES §24.2.1.
// Mirrors S152 Set narrow but with pair-arrays per entry (key, value).

// 1) ident-bound Array<Array<number>>.
const pairs: Array<Array<number>> = [[1, 10], [2, 20], [3, 30]];
const m1 = new Map(pairs);
console.log(m1.size);
console.log(m1.get(1));
console.log(m1.get(2));
console.log(m1.get(3));

// 2) `[...spread, lit-pair]` mixed source — outer-array spread shape.
const more: Array<Array<number>> = [[4, 40], [5, 50]];
const m2 = new Map([...more, [6, 60]]);
console.log(m2.size);
console.log(m2.get(4));
console.log(m2.get(6));

// 3) function-returned Array<Array<string>>.
function makePairs(): Array<Array<string>> {
  return [["a", "alpha"], ["b", "bravo"], ["c", "charlie"]];
}
const m3 = new Map(makePairs());
console.log(m3.size);
console.log(m3.get("a"));
console.log(m3.get("c"));

// 4) `[...spread]` only — outer src is fresh array we own + drop.
const src: Array<Array<number>> = [[7, 70], [8, 80]];
const m4 = new Map([...src]);
console.log(m4.size);
console.log(m4.get(7));
console.log(m4.get(8));

// 5) empty outer array.
const m5 = new Map([] as Array<Array<number>>);
console.log(m5.size);

// 6) Array<Array<string>> via ident-bound.
const strPairs: Array<Array<string>> = [["x", "X"], ["y", "Y"]];
const m6 = new Map(strPairs);
console.log(m6.size);
console.log(m6.get("x"));
console.log(m6.get("y"));
