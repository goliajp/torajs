// §28.1.1 Reflect.apply(target, thisArg, argumentsList) hands the
// receiver to the target through the same dispatch `f.apply(o, …)`
// rides, so a `this`-reading function expression in that position is
// a receiver-safe use — the promoted body's `__this` is exactly the
// thisArg. Until rotation 632 the target sat outside every admitted
// shape and the program refused to compile ("fnexpr this in
// unclaimed receiver position"), while the two member spellings of
// the same call both worked.
const o = { v: 42 };

// Inline target — the face collector's job.
console.log("inline", Reflect.apply(function () {
  return (this as any).v;
}, o, []));

// Target through a binding — the use-shape census's job.
const f = function () {
  return (this as any).v;
};
console.log("binding", Reflect.apply(f, o, []));

// The same binding used both ways stays consistent.
console.log("dot-apply", f.apply(o, []));
console.log("dot-call", f.call(o));

// Arguments still arrive after the receiver.
const g = function (a: any, b: any) {
  return (this as any).v + a + b;
};
console.log("with-args", Reflect.apply(g, o, [1, 2]));

// A non-Ident thisArg, and a target that ignores `this`.
console.log("plain", Reflect.apply(function (a: any) {
  return a * 2;
}, null, [21]));
