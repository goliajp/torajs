// chunk 787 — pins the method-call / ctor argument faces of the
// chunk-784 fill + declared-layout hint: class methods desugar to
// top-level FnDecls, so `c.m({ n: 7 })` fills the absent optional
// field and pins the param's declared layout through the same
// direct-call lanes (probes s2/s3 verified green — no separate fix
// was needed; this fixture keeps it that way).
type O = { tag?: string, n: number };
class C {
  m(o: O): string { return String(o.n) + (o.tag ?? "-") }
}
const c = new C();
console.log(c.m({ n: 7 }));
console.log(c.m({ tag: "z", n: 3 }));
type B = { v: number };
type A = { v?: number };
class E {
  ea(a: A): number { return a.v ?? 0 }
  eb(b: B): number { return b.v }
}
const e = new E();
console.log(e.eb({ v: 2 }));
console.log(e.ea({ v: 3 }));
class D {
  v: number;
  constructor(o: O) { this.v = o.n }
}
const d = new D({ n: 8 });
console.log(d.v);
