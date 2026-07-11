// RFC 20260711-closure-reflection chunk A — `<Ctor>.prototype.<m>`
// static method-value reads. The three-level Member form routes to
// `__torajs_builtin_proto_method_value(tag, key)`: singleton-dynobj
// own-entry probe first (user monkey-patch wins), then the interned
// reified method cell (chunk 711 layout — typeof "function", strict-eq
// pointer identity, `.call` re-binds the receiver), else undefined.
//
// Acceptance: byte-equal with bun.

// 1. extraction answers a function, identity is the interned singleton
const s: any = String.prototype.slice;
console.log(typeof s);
console.log(String.prototype.slice === String.prototype.slice);

// 2. per-family routing — Date annexB alias / Array / universal probe
const d: any = Date.prototype.getYear;
console.log(typeof d);
const arrHas: any = Array.prototype.hasOwnProperty;
console.log(typeof arrHas);

// 3. Function.prototype's own surface
const c: any = Function.prototype.call;
console.log(typeof c);
const b: any = Function.prototype.bind;
console.log(typeof b);

// 4. extracted cell re-binds through .call — the ecosystem idiom
console.log(s.call("hello", 1));
const own: any = Object.prototype.hasOwnProperty;
console.log(own.call({ a: 1 }, "a"), own.call({ a: 1 }, "b"));

// 5. monkey-patch entries win over the builtin cell
(String.prototype as any).custom = "mine";
console.log((String.prototype as any).custom);
(Array.prototype as any).slice = 42;
const patched: any = Array.prototype.slice;
console.log(patched);

// 6. unknown name on a supported proto stays undefined
const nope: any = Map.prototype.zzz;
console.log(nope === undefined);
