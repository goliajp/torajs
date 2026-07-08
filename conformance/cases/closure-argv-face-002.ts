// argv-face consumption audit (RFC 20260708-closure-argv-face
// chunk 2): a variadic-typed identity callback hands a fresh heap
// string through the boxed lane and back (the caller's arg-box
// ledger and the Any-ret pass-through stay balanced), and repeated
// round-trips stay flat.
function mk(n: number): string { return "value-" + n; }
function h(cb: (...args: any[]) => any): any { return cb(mk(42)); }
const r: any = h((x: any) => x);
console.log(r);
let total = 0;
for (let i = 0; i < 50; i++) {
  const v: any = h((x: any) => x);
  total += v.length;
}
console.log(total);
