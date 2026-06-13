// RFC C3b — accessor getter dispatch via Object.defineProperty.
// Reading a property defined with a `get` descriptor invokes the
// getter and yields its (NaN-box-coerced) return, dispatched off the
// AccessorPair stored in the dynobj entry. Covers the value-ABI kinds
// the runtime invoke path must box across the register-class boundary:
// I64, captured-I64, Str pointer, F64, plus the canonical test262
// closure-capture idiom.

const o: any = {};
Object.defineProperty(o, "g", {
  get() {
    return 7;
  },
});
console.log(o.g); // 7

const base = 100;
const o2: any = {};
Object.defineProperty(o2, "v", {
  get() {
    return base + 1;
  },
});
console.log(o2.v); // 101

const o3: any = {};
Object.defineProperty(o3, "s", {
  get() {
    return "tora";
  },
});
console.log(o3.s); // tora

const o4: any = {};
Object.defineProperty(o4, "f", {
  get() {
    return 3.14;
  },
});
console.log(o4.f); // 3.14

// Canonical test262 accessor idiom — getter records its invocation
// through a captured flag and the read yields the returned value.
let accessed = false;
const o5: any = {};
Object.defineProperty(o5, "p", {
  get() {
    accessed = true;
    return 42;
  },
});
const r = o5.p;
console.log(r, accessed); // 42 true
