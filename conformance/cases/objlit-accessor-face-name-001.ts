// 566-01 — an accessor face answers two different questions with two
// different spellings, and tr was giving its own fold's spelling to
// both. `{ get gg() {} }` parses into a `__getter_gg` FIELD, and that
// name went into the fn-name registry: `.name` answered
// `"__getter_gg"` and inspect printed `[Function: __getter_gg]`.
//
// bun answers `"get gg"` for the first and `[Function: gg]` for the
// second. Both are right, because they are not the same question:
// §10.2.9 SetFunctionName is called with the "get" prefix, while
// inspect reads the name in the SOURCE, where the function is
// written `gg`. So the registry row carries the source spelling and
// the §10.2.9 form is attached at the define point — which is also
// where a COMPUTED accessor key finally gets one.
//
// `Object.defineProperty(o, p, {get: f})` is not a definition site:
// §10.1.6.3 stores whatever function the caller handed over, under
// whatever name it already had.
const k = "c1";
const sD = Symbol("d");

const o: any = {
  get gg() { return 1 },
  set gg(v: number) {},
  get zz() { return 2 },
  get 7() { return 3 },
  get [k]() { return 4 },
  set [k](v: number) {},
  get [sD]() { return 5 },
};

const dg = Object.getOwnPropertyDescriptor(o, "gg")!;
console.log(JSON.stringify(dg.get!.name), JSON.stringify(dg.set!.name));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o, "zz")!.get!.name));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o, "7")!.get!.name));
const dk = Object.getOwnPropertyDescriptor(o, k)!;
console.log(JSON.stringify(dk.get!.name), JSON.stringify(dk.set!.name));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o, sD as any)!.get!.name));

// the inspect face: the source spelling, and nothing at all for a key
// the source cannot spell
console.log(dg.get, dg.set, dk.get, dk.set);

// §10.2.10 arity, and the own-key list of a face is still the pair
console.log(JSON.stringify(dg.get!.length), JSON.stringify(dg.set!.length));
console.log(JSON.stringify(Object.getOwnPropertyNames(dg.get!)));

// a descriptor's face is not renamed
const d: any = {};
Object.defineProperty(d, "p", { get: function () { return 6 }, configurable: true });
Object.defineProperty(d, "q", { get: () => 7, configurable: true });
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(d, "p")!.get!.name));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(d, "q")!.get!.name));

// and the accessors still work, still enumerable, still one entry
console.log(o.gg, o.zz, o[7], o[k], o[sD], d.p, d.q);
console.log(JSON.stringify(Object.keys(o)));
