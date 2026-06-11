// W3 C4 (rfc 20260611-ann-width-unification §5.3) — `*` -0 semantics.
// An int product is -0 when one factor is zero and the other
// negative (JS spec §13.7). A non-positive constant cofactor makes
// that reachable, so those multiplies take the float path; positive
// constant cofactors and squares keep the int path.

// S4 form: constant fold must keep the sign
console.log(0 * -1);
console.log(1 / (0 * -1));
console.log(-1 * 0);

// constant pairs that miss the zero×negative pattern stay int
console.log(3 * -4);
console.log(0 * 5);

// negation idiom over a runtime zero / nonzero value
function negate(x: number): number {
  return x * -1;
}
console.log(negate(0));
console.log(1 / negate(0));
console.log(negate(7));
console.log(negate(-7));

// zero cofactor over a runtime negative value
function zeroOut(x: number): number {
  return 0 * x;
}
console.log(zeroOut(-5));
console.log(1 / zeroOut(-5));
console.log(zeroOut(5));

// positive cofactor and square stay on the int path
function tripled(x: number): number {
  return x * 3;
}
function square(x: number): number {
  return x * x;
}
console.log(tripled(-4));
console.log(square(-9));
console.log(1 / square(0));
