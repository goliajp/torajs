// Chunk 640 — array / object literals are owned temps: consumed as
// a call argument they release after the call (pre-fix every block
// leaked — probes l22/l22b/l22c 44.8/25.5/44.8MB churn → flat), and
// owning consumers (any-box, push, field store) take the literal's
// own stake over instead of stacking an extra inc. Value behavior
// locked here; the leak faces are probe-verified.
function g(xs: number[]): number {
  return xs.length;
}
type P = { k: number };
function h(o: P): number {
  return o.k;
}
console.log(g([1, 2, 3]));
console.log(h({ k: 7 }));
// escaping literal arg — callee stores it; the container keeps the
// only stake after the call
const box: number[][] = [];
function keep(v: number[]): void {
  box.push(v);
}
keep([4, 5]);
keep([6]);
console.log(box.length);
console.log(box[0][1]);
console.log(box[1][0]);
// literal into an any slot transfers its stake
const a: any = [8, 9];
console.log(a[1]);
// literal as a bare condition (chunk 636 face) and in a ternary
if ([0]) {
  console.log("truthy");
}
console.log([1].length === 1 ? "one" : "many");
// nested literal args through a loop
let n = 0;
for (let i = 0; i < 100; i++) {
  n += g([i, i, i]);
}
console.log(n);
console.log("done");
