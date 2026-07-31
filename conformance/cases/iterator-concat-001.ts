// Iterator.concat(...items) — proposal-iterator-sequencing (刀 5a).
// Sequenced lazy iteration over mixed iterables; eager per-item
// iterability validation at construction.
const c1: any = Iterator.concat([1, 2].values(), [3, 4].values());
console.log(c1.next().value, c1.next().value, c1.next().value, c1.next().value, c1.next().done);
// array literals directly (@@iterator via the builtin reify)
console.log([...Iterator.concat([5], [6, 7])].length);
// zero items — immediately done
const c2: any = Iterator.concat();
console.log(c2.next().done);
// generator item + trailing array
function* g() {
  yield 8;
  yield 9;
}
const c3: any = Iterator.concat(g(), [10]);
console.log([...c3].join(","));
// construction opens nothing (generator body runs on first step)
let opened = 0;
function* probe() {
  opened++;
  yield 1;
}
const c4: any = Iterator.concat(probe());
console.log(opened);
c4.next();
console.log(opened);
// helper chain over concat
const c5: any = Iterator.concat([1, 2], [3]).map((x: any) => x * 2);
console.log([...c5].join(","));
// take short-circuit over concat
const c6: any = Iterator.concat([1, 2, 3].values()).take(2);
console.log([...c6].join(","));
// eager TypeError: non-object item
const bad1: any = 5;
let threw1 = false;
try {
  Iterator.concat(bad1);
} catch (e) {
  threw1 = true;
}
console.log(threw1);
// eager TypeError: string primitive item
const bad2: any = "ab";
let threw2 = false;
try {
  Iterator.concat(bad2);
} catch (e) {
  threw2 = true;
}
console.log(threw2);
// eager TypeError: plain object with no @@iterator
const bad3: any = {};
let threw3 = false;
try {
  Iterator.concat(bad3);
} catch (e) {
  threw3 = true;
}
console.log(threw3);
// Map / Set items open through their builtin @@iterator
const m = new Map([["k", 1]]);
const s = new Set([2]);
for (const x of Iterator.concat(m, s) as any) {
  console.log(Array.isArray(x) ? x[0] : x);
}
