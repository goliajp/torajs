// 567-03 — a computed accessor answers its own source text from
// `toString` (§20.2.3.5). It used to answer the erased native form,
// because the fn-name registry row is where the source slice lives
// and a computed accessor was given no row at all: it has no source
// NAME, and 566-02 read that as having nothing to record. The two
// things a row carries are separable — this one keeps the empty name
// (so the inspect face stays anonymous, which is right) and gains
// the text.
//
// The exact spelling is not compared: bun runs the transpiler's
// re-print of the file, tr slices the file itself, so the two agree
// on what the text SAYS and not on how it is laid out.

let k = "c1";
class C {
  get [k]() {
    return 2;
  }
  set [k + "s"](v: any) {}
  static get [k + "q"]() {
    return 3;
  }
  [k + "m"]() {
    return 4;
  }
}
const g = Object.getOwnPropertyDescriptor(C.prototype, k)!.get!;
const s = Object.getOwnPropertyDescriptor(C.prototype, k + "s")!.set!;
const q = Object.getOwnPropertyDescriptor(C, k + "q")!.get!;
const m = (C.prototype as any)[k + "m"];

for (const f of [g, s, q, m]) {
  const t = String(f);
  console.log(t.includes("[native code]"), t.startsWith("function"), t.includes("["));
}
console.log(String(g).startsWith("get ["), String(s).startsWith("set ["));
console.log(String(q).startsWith("get ["), String(m).startsWith("["));

// The name faces are the other question and do not move: §10.2.9's
// prefixed form on `.name`, the anonymous form on inspect (a
// computed key is in no source name position).
console.log(JSON.stringify(g.name), JSON.stringify(s.name));
console.log(JSON.stringify(q.name), JSON.stringify(m.name));
console.log(g, s, q, m);

// A source-named accessor keeps answering its own spelling on both.
class D {
  get p() {
    return 5;
  }
}
const dp = Object.getOwnPropertyDescriptor(D.prototype, "p")!.get!;
console.log(String(dp).includes("[native code]"), JSON.stringify(dp.name), dp);

// And they all still work.
const c: any = new C();
console.log(c[k], (C as any)[k + "q"], c[k + "m"]());
