// Plan A — escape analyzer for `let X = [...]` Array literals.
// Verifies the verifier correctly:
//   - rewrites the rpn-eval-shape happy case (no escape)
//   - falls back to heap path for every escape route, preserving
//     bun-equivalent output

// (1) Happy path — local stack array, only X[i] / X.length, ints only.
function happy(): number {
  let xs: number[] = [10, 20, 30, 40, 50];
  let s: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    s = s + xs[i];
  }
  return s;
}
console.log(happy());

// (2) X returned — must NOT stack-allocate (would dangle).
function escape_via_return(): number[] {
  let xs: number[] = [1, 2, 3];
  return xs;
}
let r1: number[] = escape_via_return();
console.log(r1[0] + r1[1] + r1[2]);

// (3) X passed to fn — must NOT stack-allocate (callee may store).
function consume(arr: number[]): number {
  return arr[0] + arr[1];
}
function escape_via_call(): number {
  let xs: number[] = [100, 200];
  return consume(xs);
}
console.log(escape_via_call());

// (4) X aliased to another let — must NOT stack-allocate.
function escape_via_alias(): number {
  let xs: number[] = [7, 8, 9];
  let ys: number[] = xs;
  return ys[0];
}
console.log(escape_via_alias());

// (5) X.something other than .length — must NOT stack-allocate
//     (.push would realloc out-of-stack).
function escape_via_push(): number {
  let xs: number[] = [1, 2, 3];
  xs.push(4);
  return xs[3];
}
console.log(escape_via_push());

// (6) Refcounted element type — must NOT stack-allocate (elements
//     would leak under the STATIC_LITERAL flag's drop short-circuit).
function escape_via_refcounted_elem(): number {
  let xs: string[] = ["alpha", "beta", "gamma"];
  let total: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    total = total + xs[i].length;
  }
  return total;
}
console.log(escape_via_refcounted_elem());

// (7) Mixed expressions in body — sum of products, branches, etc.
//     Should still stack-allocate.
function happy_complex(): number {
  let xs: number[] = [2, 3, 5, 7];
  let prod: number = 1;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    if (xs[i] > 2) {
      prod = prod * xs[i];
    }
  }
  return prod;
}
console.log(happy_complex());

// (8) Index-write back to X — should still stack-allocate.
function happy_index_write(): number {
  let buf: number[] = [0, 0, 0, 0];
  buf[0] = 11;
  buf[1] = 22;
  buf[2] = 33;
  buf[3] = 44;
  return buf[0] + buf[1] + buf[2] + buf[3];
}
console.log(happy_index_write());
