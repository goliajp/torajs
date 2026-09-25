// an unannotated generator whose every yield is a number (or every one
// a string) yields through that type; what next(x) sends back stays
// any, so a string sent into a number generator is still legal.
function* range(start: number, end: number) {
  let i = start;
  while (i < end) {
    yield i;
    i = i + 1;
  }
}
function* lits() {
  yield 1;
  yield -2;
  yield 3 + 4;
}
function* words(p: string) {
  yield "a";
  yield p + "!";
}
function* echo() {
  const got = yield 10;
  console.log("got", got, typeof got);
  yield 20;
}
function* none() {}

let total = 0;
for (const v of range(0, 5)) total = total + v;
console.log(total, [...lits()], [...words("hi")]);
for (const v of range(10, 100)) {
  if (v > 11) break;
  console.log("v", v);
}
const g = range(0, 1);
console.log(JSON.stringify(g.next()), JSON.stringify(g.next()), g.next().value);
const e = echo();
console.log(e.next().value, e.next("s").value, e.next().done);
const [a, b] = lits();
console.log(a + b, none().next().value, none().next().done);
const r = range(0, 3);
r.next();
for (const v of r) {
  console.log("rest", v);
  break;
}
console.log(r.next().done);
