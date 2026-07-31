// Array.from(iterable, <fn-expr>, thisArg) — the static mapFn slot
// joins the fnexpr-this channel (rotation 260): the inline fn-expr
// promotes and the from lowering's map loop threads the boxed
// thisArg ahead of (elem, i), mirroring the trio kernels. A plain
// callback keeps the eval-and-drop thisArg posture (side effects
// still fire per §23.1.2.1).
var r1 = Array.from([1, 2, 3], function (v: any, i: number) {
  return v * (this as any).k + i;
}, { k: 100 });
console.log(JSON.stringify(r1));
var r2 = Array.from("ab", function (c: any) {
  return c + (this as any).suf;
}, { suf: "!" });
console.log(JSON.stringify(r2));
var s = new Set([4, 5]);
var r3 = Array.from(s, function (v: any) {
  return v + (this as any).d;
}, { d: 1 });
console.log(JSON.stringify(r3));
var sideEffects: any[] = [];
Array.from([1], (v: number) => v, (sideEffects.push("evaluated"), {}));
console.log(JSON.stringify(sideEffects));
var plain = Array.from([7, 8], (v: number) => v * 2);
console.log(JSON.stringify(plain));
