// The ECMAScript global objects are ordinary extensible objects, so a
// name outside the surface tr models is a runtime [[Get]] answering
// undefined (§10.1.8.1) -- not a compile-time reject. test262 leans on
// both halves: it plants expandos on them and it reads names the spec
// deliberately does not define.

// Reads of names that are genuinely absent.
console.log(Math.nope, Math.NaN, Number.nope, Date.nope, Symbol.nope);
console.log(JSON.bind, Promise.then, Array.map, RegExp.indicator);
console.log(Reflect.enumerate, Reflect.consruct, String.indicator);
console.log(Math.nope === undefined, Reflect.enumerate === undefined);

// `typeof` used to guess "function" for every name a namespace did
// not classify by shape, which is only right when the name is there.
console.log(typeof Math.nope, typeof JSON.zz, typeof Reflect.zz, typeof Object.zz);
console.log(typeof Array.zz, typeof String.zz, typeof Symbol.zz, typeof Date.zz);
console.log(typeof console.nope, typeof console.log);

// Expando write, then read it back through the same object.
Math.prop = 7;
Array.myproperty = 1;
console.log(Math.prop, Array.myproperty);
console.log(typeof Math.prop, typeof Math.nope);

// ... and through an alias, since the write lands on the singleton.
const j = JSON;
j.zzz = 5;
console.log(j.zzz, JSON.zzz);

// Object.defineProperty is the shape test262 actually uses.
Object.defineProperty(JSON, "dp", { value: 42, writable: true });
console.log(JSON.dp);

// Names tr DOES model keep resolving to the builtin, unchanged.
console.log(Math.max(1, 2), Math.PI, Math.floor(1.7), Math.hypot(3, 4));
console.log(Number.MAX_SAFE_INTEGER, Number.EPSILON, Number.isInteger(3));
console.log(Array.from("ab"), Array.isArray([]), Array.name, Array.length);
console.log(JSON.stringify({ a: 1 }), Object.keys({ a: 1 }));
console.log(String.fromCharCode(65), typeof Array.prototype);
console.log(typeof Math.max, typeof Math.PI, typeof Symbol.iterator, typeof Array.name);

// `Function` is itself a function, so it inherits the prototype
// methods -- these are hits, not misses.
console.log(typeof Function.call, typeof Function.apply, Function.constructor === Function);
