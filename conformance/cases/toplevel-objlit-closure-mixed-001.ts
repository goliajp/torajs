// A mixed object literal the __inlobj arm refuses (closure beside a
// null / nested field) promotes to the Any lane, so named-fn reads
// and calls resolve — the 546-01 face-2 shapes.

const o = { v: () => 42, w: null };
function callV() {
  console.log(o.v());
  console.log(o.w);
}
callV();
console.log(o.v());

// capture of a mutable top-level, mutation observed across fns
let total = 0;
const c = { bump: () => {
  total += 3;
  return total;
}, tag: null };
function drive() {
  console.log(c.bump());
  console.log(c.bump());
}
drive();
console.log(total);

// closure beside a nested literal (inlobj-refused mix, no receiver)
const n = {
  box: { d: 1 },
  read: () => 32,
};
function pull() {
  console.log(n.read());
  console.log(n.box.d);
}
pull();
