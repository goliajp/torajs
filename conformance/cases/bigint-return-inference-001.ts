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

// The UNANNOTATED form takes the same answer through the auto-generic
// type vars. `a` and `b` carry `__T0` / `__T1` there, and a type var
// CAN be a BigInt at the call site — so claiming `number` was a
// promise the inference had no way to keep: `f(7n, 2n)` printed the
// pointer for `- * |`, and `/ % **` failed in the register allocator.
// `+` already declined that pair, which is why it was the one
// operator that worked.
function usub(a: any, b: any) {
  return a - b;
}
function umul(a: any, b: any) {
  return a * b;
}
function udiv(a: any, b: any) {
  return a / b;
}
function urem(a: any, b: any) {
  return a % b;
}
function upow(a: any, b: any) {
  return a ** b;
}
function ubor(a: any, b: any) {
  return a | b;
}
function uneg(a: any) {
  return -a;
}
console.log(usub(7n, 2n), umul(7n, 2n), udiv(7n, 2n), urem(7n, 2n));
console.log(upow(7n, 2n), ubor(6n, 3n), uneg(7n));

// the same functions over Numbers keep answering Numbers
console.log(usub(7, 2), umul(7, 2), udiv(7, 2), urem(7, 2));
console.log(upow(7, 2), ubor(6, 3), uneg(7));
