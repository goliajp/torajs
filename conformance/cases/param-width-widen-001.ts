// L3a-8 — an annotated-`number` param assigned a statically-f64 RHS
// (`n = n / 2`) widens its I64-default slot to F64 at declaration
// (ast_refs::fn_param_widens_to_f64, shared by the signature and
// param_setup sites). Distilled collatz shape: the assignment lives in
// an if-branch inside a while loop, and i64 call args coerce via
// SiToFp at the call site.
function steps(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
    count = count + 1;
  }
  return count;
}
console.log(steps(27));
console.log(steps(1));

function halve(n: number): number {
  let r: number = 0;
  while (n > 1) {
    n = n / 2;
    r = r + 1;
  }
  return r;
}
console.log(halve(1024));
console.log(halve(7));
