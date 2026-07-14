// RFC 20260714-objlit-accessor blade 7 — a struct accessor's [[Set]]
// through an `any` receiver (the write mirror of blade 5's read). The
// struct arm of the member write used to reject outright: "cannot assign
// to a property of this any value".

const o = {
  a: 1,
  get v(): number {
    return this.a;
  },
  set v(n: number) {
    this.a = n;
  },
};
const oa: any = o;
oa.v = 42;
console.log(o.a, oa.v);

// the setter's `this` is the receiver, and a heap value crosses the call
const s = {
  s: "x",
  get label(): string {
    return this.s;
  },
  set label(v: string) {
    this.s = v + "!";
  },
};
const sa: any = s;
sa.label = "hi";
console.log(s.s, sa.label);

// class accessor: prototype-level, so it rides the dispatch table
class C {
  a: number = 1;
  get b(): number {
    return this.a;
  }
  set b(n: number) {
    this.a = n * 2;
  }
}
const c = new C();
const ca: any = c;
ca.b = 21;
console.log(c.a, ca.b);

// a GET-ONLY property refuses the write (ES §10.1.9 — an assignment
// whose [[Set]] is undefined fails, and a module is strict).
const g = {
  a: 1,
  get v(): number {
    return this.a;
  },
};
const ga: any = g;
try {
  ga.v = 9;
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}
console.log(g.a);
