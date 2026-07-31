// Knife-4 widening (rotation 260): an INLINE array-literal receiver
// (`[1,2].forEach(<fn-expr>, thisArg)`) is the arraylit_recvs shape
// without the binding hop — the same inlined-loop trio threads the
// same thisArg, so the fn-expr's `this` promotes identically.
// Pre-fix the face only matched Ident receivers and the fn-expr's
// `__this` stayed an unresolvable capture.
var out: any[] = [];
var t = { tag: 7 };
[10, 20].forEach(function (v: any) {
  out.push(v + (this as any).tag);
}, t);
console.log(JSON.stringify(out));
var m = [1, 2].map(function (v: any) {
  return v * (this as any).k;
}, { k: 3 });
console.log(JSON.stringify(m));
var f2 = [5, 6].filter(function (v: any) {
  return v > (this as any).min;
}, { min: 5 });
console.log(JSON.stringify(f2));
var count = 0;
[1, 2, 3].forEach(function () {
  count++;
}, { unused: true });
console.log(count);
