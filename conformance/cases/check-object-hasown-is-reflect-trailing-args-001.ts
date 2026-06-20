// ES §20.1.2.{4,9} / §28.1.{6,9} — Object.{hasOwn,is} +
// Reflect.{has,get} (obj, key|b, ...trailing) — trailing args
// silent-drop per spec generic trailing-arg ignore.
//
// Reflect.get's 3rd `receiver` arg is spec-meaningful only for
// Proxy targets (tora has no Proxy substrate); silent-drop is
// spec-correct for the non-Proxy case tora supports.

const o = { a: 1, b: 2 };

// Object.hasOwn — trailing arg dropped, returns true.
console.log(Object.hasOwn(o, "a"));
console.log(Object.hasOwn(o, "a", 99));
console.log(Object.hasOwn(o, "b", "ignored", 42));

// Object.is — trailing arg dropped, returns same as 2-arg call.
console.log(Object.is(1, 1));
console.log(Object.is(1, 1, "extra"));
console.log(Object.is(2, 3, 99, "ignored"));

// Reflect.has — trailing arg dropped.
console.log(Reflect.has(o, "a"));
console.log(Reflect.has(o, "a", "extra"));
console.log(Reflect.has(o, "c", 99));

// Reflect.get — trailing receiver arg dropped (non-Proxy spec
// semantics: receiver only matters for accessor properties).
console.log(Reflect.get(o, "a"));
console.log(Reflect.get(o, "a", o));
console.log(Reflect.get(o, "b", o, "extra"));
