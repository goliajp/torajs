// bug-327 third value shape: a namespace-static fn value handed into a
// fn-typed param (test262 S12.9_A4 `DD_operator(Math.sin, ...)` shape).
// The value lowers to the interned dispatcher cell (Type::Closure), so
// the receiving param must take the env-first ABI — pre-fix it kept the
// bare-fn-pointer lane and blr'd the cell's heap header.
function apply1(f: (n: number) => number, x: number): number {
  return f(x);
}
console.log(apply1(Math.sin, 0));
console.log(apply1(Math.abs, -5));
console.log(apply1((n: number) => n * 2, 21));

function pick(f: (a: number, b: number) => number, a: number, b: number): number {
  return f(a, b);
}
console.log(pick(Math.max, 3, 9));
console.log(pick((a: number, b: number) => a - b, 10, 4));
