// RFC 20260802-yield-expr-hoist — expression-position yield: return
// operand, call argument, binop operand, nested yield-yield, throw
// operand, template interpolation, array elements, paren-wrapped init.
function* g1(): any {
  return yield 1;
}
const i1 = g1();
console.log(i1.next().value);
console.log(i1.next(7).value);

function* g2() {
  console.log("arg", yield 1);
}
const i2 = g2();
i2.next();
i2.next(5);

function* g3() {
  const x = 1 + (yield 2);
  console.log(x);
}
const i3 = g3();
i3.next();
i3.next(10);

function* g4() {
  yield yield 1;
}
const i4 = g4();
console.log(i4.next().value);
console.log(i4.next(3).value);

function* g5() {
  throw yield 1;
}
const i5 = g5();
i5.next();
try {
  i5.next(new Error("boom"));
} catch (e: any) {
  console.log("caught", e.message);
}

function* g6() {
  const s = `v=${yield 1}`;
  console.log(s);
}
const i6 = g6();
i6.next();
i6.next("Z");

function* g7() {
  const a = [yield 1, yield 2];
  console.log(a[0], a[1]);
}
const i7 = g7();
i7.next();
i7.next("x");
i7.next("y");

function* g8() {
  const t = (yield 1);
  console.log(t);
}
const i8 = g8();
i8.next();
i8.next("Z");
