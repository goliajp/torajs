// A class's [Symbol.iterator]() lives on its prototype under the
// symbol; for-of / spread over a re-parented instance use its chain.
class D { *[Symbol.iterator]() { yield "d1"; yield (this as any).k; } }
console.log(typeof (D.prototype as any)[Symbol.iterator]);
const o: any = Object.create(D.prototype);
o.k = "ok";
console.log([...o].join(","));
class C2 {
  *[Symbol.iterator]() { yield "c1"; yield "c2"; }
  m() { return "C.m"; }
}
class D2 {
  *[Symbol.iterator]() { yield "d1"; }
  m() { return "D.m"; }
}
const c = new C2();
Object.setPrototypeOf(c, D2.prototype);
const out: string[] = [];
for (const v of c) out.push(v);
console.log(out.join(","), [...c].join(","));
console.log(c.m?.(), (c as C2 | undefined)?.m());
const f = c.m;
console.log(f.call(c));
