// Iterator.zipKeyed — proposal-joint-iteration (刀 5c). No bun/node
// reference implements it (probed 2026-07-31); acceptance is this
// hand-derived expectation diffed against tr run AND the AOT binary.
const in1: any = { a: [1, 2], b: [3, 4] };
const zk1: any = Iterator.zipKeyed(in1);
const r1 = zk1.next().value;
console.log(r1.a, r1.b);
const r2 = zk1.next().value;
console.log(r2.a, r2.b);
console.log(zk1.next().done);
const in2: any = { a: [1, 2], b: [9] };
const pad2: any = { b: "p" };
const zk2: any = Iterator.zipKeyed(in2, { mode: "longest", padding: pad2 });
for (const row of zk2) {
  console.log(row.a, row.b);
}
const in3: any = { a: [1], b: [2, 3] };
const zk3: any = Iterator.zipKeyed(in3, { mode: "strict" });
zk3.next();
let threw = false;
try {
  zk3.next();
} catch (e) {
  threw = true;
}
console.log(threw);
const bad: any = 5;
let threw2 = false;
try {
  Iterator.zipKeyed(bad);
} catch (e) {
  threw2 = true;
}
console.log(threw2);
const emptyObj: any = {};
console.log(Iterator.zipKeyed(emptyObj).next().done);
const in4: any = { x: [5, 6] };
const zk4: any = Iterator.zipKeyed(in4).map((r: any) => r.x * 2);
console.log([...zk4].join(","));
