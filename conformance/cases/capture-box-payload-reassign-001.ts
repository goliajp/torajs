// A module binding captured by a closure built on every loop turn and
// reassigned from inside it. Each closure's release drops one stake on
// the shared box while the box survives; the object the box held then
// is released by the next reassignment, and the program must still
// exit cleanly.
let g = { t: 0 };
for (let i = 0; i < 3; i++) {
  const f = () => { g = { t: i }; };
  f();
}
console.log(g.t);
