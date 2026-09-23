// A computed key that is a whole string literal folds to a static name
// at parse time; its METHOD SHORTHAND body must still parse for real.
// It used to be walked paren/brace-balanced and dropped for a stub, so
// the key was present with a non-function value.
const o1: any = { ["b"]() { return this.v; }, v: 7 };
console.log(o1.b(), o1.b.name, o1.b.length, typeof o1.b);

// Arity, own-key order next to plain and data-valued computed fields.
const o2: any = { ["y"]: 1, ["z"](a: any, b: any) { return a + b; }, w() { return 3; } };
console.log(Object.getOwnPropertyNames(o2).join(","), o2.z(1, 2), o2.z.length, o2.w());

// `["__proto__"]()` is an own property, not the proto setter (§B.3.1
// covers `__proto__:` PropertyDefinition only).
const o3: any = { ["__proto__"]() { return 1; } };
console.log(typeof o3.__proto__, Object.getOwnPropertyNames(o3).join(","));

// super, generator and async forms all ride the same shared tail.
const base: any = { m() { return "base"; } };
const o4: any = { __proto__: base, ["m"]() { return "sub+" + super.m(); } };
console.log(o4.m());

const o5: any = { *["g1"]() { yield 1; yield 2; } };
console.log(typeof o5.g1, [...o5.g1()].join(","));

const o6: any = { async ["c"]() { return 9; } };
o6.c().then((r: any) => console.log("async", r));
