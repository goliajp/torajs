// RFC 20260730-iterator-global 刀 2c — iterator cells' method-VALUE
// read face: typeof / identity / call-with-receiver on next and the
// helper family; `return` reads undefined off array/map iterators
// (§23.1.5 / §24.1.5 have none) and a function off helper cells
// (§27.1.5.2); the for-of / destructuring close tier stays a silent
// no-op on iterators without `return` (the reverted first cut's
// 3-fail shape).

function* g() {
  yield 1;
  yield 2;
}

const it: any = [10, 20].values();
console.log(typeof it.next);
console.log(it.next === it.next);
console.log(typeof it.map);
console.log(typeof it.toArray);
console.log(it.return);

const m = new Map();
m.set("a", 1);
const mit: any = m.entries();
console.log(typeof mit.next);
console.log(mit.return);

const h: any = it.map((v: any) => v * 2);
console.log(typeof h.next);
console.log(typeof h.return);
console.log(typeof h.flatMap);

// Extracted method re-binds through .call (§ the unbound builtin
// contract): n.call(it) steps the ArrIter underneath the helper.
const n = it.next;
console.log(JSON.stringify(n.call(it)));

// The helper sees the remaining element through its own extracted
// toArray.
const f = h.toArray;
console.log(JSON.stringify(f.call(h)));

// A bare extracted call has no receiver — spec TypeError.
try {
  n();
} catch (e: any) {
  console.log(e instanceof TypeError);
}

// Close-tier regression face (the reverted cut broke these): for-of
// early break and array destructuring over builtin iterators owe a
// close that must stay a no-op here.
const s = new Set();
s.add(1);
s.add(2);
s.add(3);
const [x, y] = s as any;
console.log(x, y);
for (const v of m as any) {
  console.log(JSON.stringify(v));
  break;
}
const gen: any = g();
console.log(typeof gen.next, typeof gen.return);
