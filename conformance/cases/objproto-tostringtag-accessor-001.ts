// §20.1.3.6 step 15 is a real Get, so an accessor-shaped
// @@toStringTag has to run its getter. The symbol probe answers an
// ACCESSOR sentinel — correct, and the emitted GET path resolves it
// (a direct `o[Symbol.toStringTag]` read always ran the getter) — but
// Object.prototype.toString read that sentinel as "not a heap value"
// and gave up, so every accessor form answered [object Object] with
// the getter never running, exception forms included.

// own accessor
const o1: any = {};
Object.defineProperty(o1, Symbol.toStringTag, {
  get() { console.log("GET own"); return "Tagged"; },
});
console.log(Object.prototype.toString.call(o1)); // GET own / [object Tagged]

// inherited accessor
const proto: any = {};
Object.defineProperty(proto, Symbol.toStringTag, {
  get() { return "Inherited"; },
});
console.log(Object.prototype.toString.call(Object.create(proto))); // [object Inherited]

// class getter syntax
class C {
  get [Symbol.toStringTag]() { return "Klass"; }
}
console.log(Object.prototype.toString.call(new C())); // [object Klass]

// a throwing getter propagates instead of being swallowed
const o2: any = {};
Object.defineProperty(o2, Symbol.toStringTag, {
  get() { throw new Error("boom"); },
});
try {
  console.log(Object.prototype.toString.call(o2));
} catch (e: any) {
  console.log("caught", e.message);
} // caught boom

// a non-String answer falls through to the builtin tag
const o3: any = {};
Object.defineProperty(o3, Symbol.toStringTag, { get() { return 42; } });
console.log(Object.prototype.toString.call(o3)); // [object Object]

// repeated reads stay correct (the getter's cell is released each time)
const o4: any = {};
Object.defineProperty(o4, Symbol.toStringTag, { get() { return "X"; } });
console.log(
  Object.prototype.toString.call(o4),
  Object.prototype.toString.call(o4),
  Object.prototype.toString.call(o4),
); // [object X] [object X] [object X]

// data-property and builtin forms are untouched
const o5: any = {};
Object.defineProperty(o5, Symbol.toStringTag, { value: "Data" });
console.log(Object.prototype.toString.call(o5)); // [object Data]
console.log(Object.prototype.toString.call(new Map())); // [object Map]
