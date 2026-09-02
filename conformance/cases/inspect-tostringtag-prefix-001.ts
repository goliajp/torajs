// 561-02 / 561-03 — the name bun prints in front of an ordinary
// object's `{`: JSC's calculatedClassName (the `constructor` data
// property's function name, own first then up the chain; when that
// is missing or `Object`, a String `@@toStringTag` reached by Get),
// and bun's fast / slow property walk deciding whether the tag key
// itself prints as a row (`bindings.cpp` forEachProperty: the fast
// structure walk hides constructor / __proto__ / @@toStringTag;
// an accessor, an index key, an own __proto__, or a fast walk that
// prints nothing switch to the slow walk, which hides only the
// non-enumerable ones). Properties inherited from a user prototype
// (bun walks up to five of them) are a separate gap (562-05).
const o: any = { a: 1, [Symbol.toStringTag]: "Tagged" };
console.log(o);
console.log([o]);
console.log({ inner: o });
class X { v = 1; }
const x: any = new X(); x[Symbol.toStringTag] = "T";
console.log(x);
const num: any = { a: 1, [Symbol.toStringTag]: 5 };
console.log(num);
const emp: any = { [Symbol.toStringTag]: "" };
console.log(emp);
const sp: any = { [Symbol.toStringTag]: "has space", b: 2 };
console.log(sp);
const arr: any = [1, 2]; arr[Symbol.toStringTag] = "AT";
console.log(arr);
const nested: any = { a: { [Symbol.toStringTag]: "Deep", z: 0 } };
console.log(nested);
const only: any = { [Symbol.toStringTag]: "T" };
console.log(only);
const acc: any = { get x() { return 1; }, [Symbol.toStringTag]: "T2" };
console.log(acc);
const idx: any = { 0: 1, [Symbol.toStringTag]: "T3" };
console.log(idx);
class K { m() {} }
console.log(K.prototype);
const ctor: any = { constructor: function Foo() {}, a: 1 };
console.log(ctor);
const objtag: any = { [Symbol.toStringTag]: "Object", a: 1 };
console.log(objtag);
const np: any = Object.create(null); np[Symbol.toStringTag] = "Object";
console.log(np);
const n2: any = Object.create(null); n2[Symbol.toStringTag] = "N"; n2.q = 3;
console.log(n2);
const dne: any = { a: 1 }; Object.defineProperty(dne, Symbol.toStringTag, { value: "DNE", enumerable: false });
console.log(dne);
const e: any = {}; Object.defineProperty(e, Symbol.toStringTag, { value: "E" });
console.log(e);
const protoq: any = { a: 1 }; Object.defineProperty(protoq, "__proto__", { value: 5, enumerable: true });
console.log(protoq);
