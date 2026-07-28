// S2.42 (rotation 240) — the sibling-class static dispatch lane
// handed call arguments verbatim (no arg_conv): with two generator
// declarations `next` becomes a sibling-owned name, `g.next(42)`
// routed through that lane, and the resumption value read back as a
// garbage NaN-box ([unknown-any-tag]).
function* h1() {
  const v = yield 1;
  console.log("h1 got", v);
}
function* h2() {
  const w = yield 9;
  console.log("h2 got", w);
  const x = yield 10;
  console.log("h2 then", x);
}
const a = h1();
a.next();
a.next(42);
const b = h2();
b.next();
b.next("str");
b.next(2.5);
