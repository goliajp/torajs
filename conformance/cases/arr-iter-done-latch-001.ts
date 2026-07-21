// §23.1.5.2.1 — an exhausted array iterator latches done: pushing
// onto the source afterwards must not revive it (RFC
// 20260721-array-proto-cluster 刀 1 / G6).
const a: any[] = [1];
const vals = a.values();
console.log(vals.next().done); // false
console.log(vals.next().done); // true — exhausted, latch set
a.push(2);
const revived = vals.next();
console.log(revived.done, revived.value); // true undefined

const b: any[] = [10];
const ks = b.keys();
ks.next();
ks.next();
b.push(20);
console.log(ks.next().done); // true

const c: any[] = ["x"];
const es = c.entries();
es.next();
es.next();
c.push("y");
const er = es.next();
console.log(er.done, er.value); // true undefined

// A live iterator still tracks growth before exhaustion.
const d: any[] = [1];
const live = d.values();
console.log(live.next().value); // 1
d.push(2);
console.log(live.next().value); // 2
console.log(live.next().done); // true
