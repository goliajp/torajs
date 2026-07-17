// RFC 20260717-objlit-anylane-recv knife 1 — any-lane object-literal
// recv members. A this-using method/accessor in a dynobj-lane literal
// promotes to the `__this: any` receiver-first shape (the nominal
// `__ObjLit_n` stamp reads struct offsets off the dynobj receiver —
// the pre-fix this-using method SIGSEGV'd); accessor shorthand fields
// install live AccessorPair entries instead of dead data fields.

// this-using method through any dispatch
const o: any = { v: 7, f() { return this.v; }, plain() { return 100; } };
console.log(o.f()); // 7
console.log(o.plain()); // 100 (this-free method keeps the plain ABI)

// getter reading this
const p: any = { w: 3, get b() { return this.w; } };
console.log(p.b); // 3

// setter writing this + get/set pair on one prop
const q: any = {
  _x: 0,
  set x(nv) {
    this._x = nv * 2;
  },
  get x() {
    return this._x + 1;
  },
};
q.x = 21;
console.log(q._x, q.x); // 42 43

// this-free empty getter answers undefined but OWNS a descriptor
// (test262 verifyProperty-undefined-desc shape); the literal-undefined
// field routes the unannotated binding through the any lane
let sample = { bar: undefined, get baz() {} };
console.log(sample.baz); // undefined
console.log(Object.getOwnPropertyDescriptor(sample, "baz") !== undefined); // true

// method args shift correctly past the prepended receiver
const r: any = { base: 10, add(a, b) { return this.base + a + b; } };
console.log(r.add(1, 2)); // 13
console.log("done");
