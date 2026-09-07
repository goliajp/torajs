// §25.5.2.4 step 5 — a PropertyList replacer walks the listed keys
// through `Get`, so it must see exactly what a plain read sees.
//
// The probe pair behind that walk handled only one of the two accessor
// forms: it invoked an AccessorPair cell directly and took no key at
// all, which left the struct form — no pair cell, addressed by key,
// the value channel answering zero on purpose — unreachable.

const rep = { a: 1, get b() { return 2; } };
const w: any = rep;
console.log(w.b, JSON.stringify(w), JSON.stringify(w, ["b"]), JSON.stringify(w, ["b", "a"]));

// A class accessor is the prototype's, and PropertyList uses Get, so
// it appears here while the own-properties walk leaves it out.
class C {
  a = 1;
  get b() {
    return 2;
  }
}
const c: any = new C();
console.log(JSON.stringify(c), JSON.stringify(c, ["b", "a"]));

// The getter sees its receiver, and a missing key contributes nothing.
class D {
  a = 3;
  get sum() {
    return this.a + 10;
  }
}
const d: any = new D();
console.log(JSON.stringify(d, ["sum", "a", "absent"]));

// The dynobj AccessorPair form, which always worked.
const dy: any = {};
Object.defineProperty(dy, "b", { get: function () { return 2; }, enumerable: true });
dy.a = 1;
console.log(JSON.stringify(dy), JSON.stringify(dy, ["b"]));

// A throwing getter propagates out of the list walk too.
const t: any = { get x() { throw new TypeError("boom"); } };
try {
  JSON.stringify(t, ["x"]);
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
