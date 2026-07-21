// Object.hasOwn / Reflect.has on a struct-lane literal with accessor
// properties: either accessor half is an own property (§10.4), and
// the synthetic half-slot spellings never leak as user-visible keys.
let o = { get foo() { return 1; } };
console.log(Object.hasOwn(o, "foo"), Object.hasOwn(o, "__getter_foo"), Object.hasOwn(o, "absent"));
let os = { set only(v: any) {} };
console.log(Object.hasOwn(os, "only"));
let om = { a: 1, get b() { return 2; } };
console.log(Reflect.has(om, "a"), Reflect.has(om, "b"), Reflect.has(om, "__getter_b"));
