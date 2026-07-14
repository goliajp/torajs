// RFC 20260714-objlit-accessor blade 5 — a struct accessor reached
// through an `any` receiver. Blades 1-4 taught the reflection surface
// that an accessor is an own property; the value face through `any`
// still read `undefined`.

// object-literal accessor: the getter closure is a layout field, and
// the receiver has to reach it as `this`.
const o = {
  a: 1,
  get v(): number {
    return 2;
  },
  get w(): number {
    return this.a + 10;
  },
};
const oa: any = o;
console.log(oa.v, oa.w);

// [[Get]] runs EXACTLY once per read — the (tag, value) probe pair is
// two kernel calls, so neither channel may invoke the getter.
const s = {
  n: 0,
  get probe(): number {
    this.n = this.n + 1;
    return this.n;
  },
};
const sa: any = s;
const r1 = sa.probe;
const r2 = sa.probe;
console.log(r1, r2, s.n);

// heap-typed getter result, and a set-only property reads `undefined`
// (ES §10.1.8 — an accessor whose [[Get]] is undefined).
const g = {
  s: "hi",
  get greet(): string {
    return "hello " + this.s;
  },
  set only(n: number) {
    this.s = "set";
  },
};
const ga: any = g;
console.log(ga.greet, ga.only);

// class accessor: prototype-level, so it rides the class dispatch table
// instead of a layout slot. Override + `this` through the same lane.
class B {
  a: number = 5;
  get tag(): string {
    return "base";
  }
}
class D extends B {
  get tag(): string {
    return "derived" + this.a;
  }
}
const bd: any = new D();
const bb: any = new B();
console.log(bd.tag, bb.tag);

// a getter that throws propagates through the `any` lane.
const t = {
  get boom(): number {
    throw new Error("bang");
  },
};
const ta: any = t;
try {
  console.log(ta.boom);
} catch (e) {
  console.log("caught:", (e as Error).message);
}

// the synthetic slot spelling is neither a property nor a method: the
// property is `v` / `tag`, never `__getter_v` / `tag_get`.
const da: any = new D();
console.log(oa.__getter_v, typeof da.tag_get);
