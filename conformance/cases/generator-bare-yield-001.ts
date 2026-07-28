// S2.41 — YieldExpression's operand is optional (ES §15.5.5): a bare
// `yield` before a terminator yields undefined. The t262 for-of /
// for-await-of dstr suites drive every generator with `yield;` (a
// 75-case parse wall: "expected expression, got Semi").
function* g() {
  yield;
  yield 2;
  yield;
}
const it = g();
console.log(it.next());
console.log(it.next());
console.log(it.next());
console.log(it.next());
function* counters() {
  let first = 0;
  first += 1;
  yield;
  first += 10;
  yield;
  console.log("first:", first);
}
const c = counters();
c.next();
c.next();
c.next();
for (const v of g()) {
  console.log("saw", v);
}
