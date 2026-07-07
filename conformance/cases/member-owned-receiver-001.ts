// Chunk 637 — member READ on an owned receiver temp: the receiver
// (Call / New / As-of-Call result) must release after the field
// detaches, and the detached result must carry exactly one stake
// through binding / assignment / argument consumers. Value behavior
// locked here; the leak faces are probe-verified (l16f/l16i/l16o/
// l16p: 25.6MB churn → flat).
class K {
  x: number;
  s: string;
  constructor(x: number) {
    this.x = x;
    this.s = "payload-" + x;
  }
}
function mk(x: number): K {
  return new K(x);
}
// New receiver, copy field
console.log(new K(1).x);
// Call receiver, heap field into a binding
const v = mk(2).s;
console.log(v);
// Call receiver, chained consumer
console.log(mk(3).s.length);
// assignment form re-targets an existing slot
let w = "seed";
w = mk(4).s;
console.log(w);
// as-cast receiver (Any lane)
const k5 = new K(5);
const wr = new WeakRef<K>(k5);
console.log((wr.deref() as K).x);
console.log((wr.deref() as K).s);
// owned receiver consumed as a bare condition (chunk 636 face)
if (mk(6)) {
  console.log("cond ok");
}
// loop churn keeps values correct while receivers recycle
let n = 0;
for (let i = 0; i < 100; i++) {
  n += mk(i).s.length;
}
console.log(n);
