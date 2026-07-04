// L3b #8 (chunk 525) — the remaining FnSig-into-any positions
// beyond let-init (chunk 518) and any-init containers (chunk 519):
// assign into an any-declared binding, a bare fn passed to an
// any-annotated param, and `return fn` from an any-returning fn.
// Each position wraps the bare top-FnDecl Ident into its
// `__forward_*` zero-capture closure so the any box holds a
// closure cell with a boxed dual entry.
function add(a: number, b: number): number { return a + b; }
function mul(a: number, b: number): number { return a * b; }
let f: any;
f = add;
console.log(f(1, 2));
f = mul;
console.log(f(3, 4));
function callIt(g: any): number { return g(3, 4); }
console.log(callIt(add));
function pick(flag: boolean): any {
  if (flag) {
    return add;
  }
  return mul;
}
const h = pick(true);
console.log(h(5, 6));
const m = pick(false);
console.log(m(5, 6));
function second(tag: string, g: any): number { return g(10, 20); }
console.log(second("x", add));
function direct(a: number, b: number): number { return a + b; }
console.log(direct(7, 8));
console.log(typeof f);
