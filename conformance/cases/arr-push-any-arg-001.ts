// TS any-assignability at the push/unshift 1-arg boundary: an Any arg
// (any-arith result or bare any binding) into a Number/String elem
// array admits and unboxes at the store (previously a loud checker
// reject; admitting without the coerce would raw-write NaN-box bits).
// Covers ident, field, and unshift receivers plus the multi-arg
// desugared form and the mapset-thisArg composed shape.
const out: number[] = [];
const a: any = 5;
out.push(a * 2);
out.push(a, 7);
console.log(out.join(","));
const s: string[] = [];
const b: any = "x";
s.unshift(b + "y");
s.push(b);
console.log(s.join(","));
class W {
  items: number[] = [];
}
const w = new W();
const c: any = 3;
w.items.push(c * 7);
console.log(w.items[0]);
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const acc: number[] = [];
m.forEach(function (v: number) {
  acc.push(v * this.mul);
}, { mul: 10 });
console.log(acc.join(","));
