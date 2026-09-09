// §6.1.6.2 — the BigInt operators answer a BigInt, not a Number. The
// implicit return-annotation inference answered `"number"` for the
// whole arithmetic family regardless of operand type, so an
// unannotated-RETURN function over declared `bigint` parameters was
// refused outright:
//
//     function f(a: bigint, b: bigint) { return a - b; }
//         type error: return type mismatch: function expects Number,
//                     got BigInt
//
// `+` was the one operator that escaped, because its arm already read
// its operands. Writing the return annotation by hand also worked —
// one question, two answers.

function sub(a: bigint, b: bigint) {
  return a - b;
}
function mul(a: bigint, b: bigint) {
  return a * b;
}
function div(a: bigint, b: bigint) {
  return a / b;
}
function rem(a: bigint, b: bigint) {
  return a % b;
}
function pow(a: bigint, b: bigint) {
  return a ** b;
}
function bor(a: bigint, b: bigint) {
  return a | b;
}
function shl(a: bigint, b: bigint) {
  return a << b;
}
console.log(sub(7n, 2n), mul(7n, 2n), div(7n, 2n), rem(7n, 2n));
console.log(pow(7n, 2n), bor(6n, 3n), shl(3n, 2n));

// §13.5.5 / §13.5.7 — unary `-` and `~` over a BigInt answer a BigInt
function neg(a: bigint) {
  return -a;
}
function bnot(a: bigint) {
  return ~a;
}
console.log(neg(7n), bnot(7n));

// a BigInt literal on one side names the family just as well
function dec(a: bigint) {
  return a - 1n;
}
console.log(dec(7n));

// arrow bodies take the same inference
const arrow = (a: bigint, b: bigint) => a - b;
console.log(arrow(7n, 2n));

// the shapes that already worked stay working
function add(a: bigint, b: bigint) {
  return a + b;
}
function lt(a: bigint, b: bigint) {
  return a < b;
}
function declared(a: bigint, b: bigint): bigint {
  return a - b;
}
console.log(add(7n, 2n), lt(7n, 2n), declared(7n, 2n));

// a Number return still names number
function num(a: number, b: number) {
  return a * b - 1;
}
console.log(num(3, 4));
