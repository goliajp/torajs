// §23.1.3.13 — flatMap flattens exactly one level, so a callback
// answering `Array<Array<V>>` products an `Array<Array<V>>` whose
// elements are the inner arrays themselves (pointer slots, same walk
// as Str inners).
const xs = [1, 2, 3];
const nested = xs.flatMap((n) => [[n, n]]);
console.log(nested.length, nested[0][1], nested[2][0]);
const strs = ["a", "bb"];
const pairs = strs.flatMap((s) => [[s]]);
console.log(pairs.length, pairs[1][0]);
