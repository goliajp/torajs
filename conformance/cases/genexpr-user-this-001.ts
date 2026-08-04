// r295 — a generator EXPRESSION body's `this` is the factory call's
// receiver (§27.5.1.1 OrdinaryCallBindThis), riding the class
// generator method's `__genrecv` channel: the parse-time mint turns
// body `this` into the leading receiver param, the generator desugar
// turns that param into a `__Gen_*` field, and the wrap forwarder's
// cell carries FLAG_CLOSURE_RECV_FIRST so a method-shaped call seeds
// the receiver into argv[0]. Pre-fix the body `this` resolved to the
// state-machine instance ("no member `.length` on ClassRef" — the
// t262 dstr ary-ptrn-elem-id-iter-val-array-prototype family).
const holder: any = { length: 2, 0: "a", 1: "b" };
holder[Symbol.iterator] = function* () {
  if ((this as any).length > 0) { yield (this as any)[0]; }
  if ((this as any).length > 1) { yield (this as any)[1]; }
};
const it = holder[Symbol.iterator]();
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().done);

// this-free generator expressions keep the no-receiver factory shape
const g2 = (function* () { yield 42; })();
console.log(g2.next().value);

// member-store spelling seeds the receiver the same way
const box: any = {};
box.gen = function* () { yield (this as any).tag; };
box.tag = "boxed";
const g3 = box.gen();
console.log(g3.next().value);
