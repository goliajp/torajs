// A generator local holding an object literal with a computed key (or
// a `__proto__: v` field) is a dynobj, and its lifted class field has
// to say so: the field-annotation sniff spelled the literal as the
// struct of its named fields, the factory's drop walk read a Str off
// the dynobj at that layout, and the program died at exit
// (SIGSEGV in `__torajs_str_drop`; rotation 640).
function* g(key: any) { let o = { [key]: 1, a: 'a' }; yield 1; return o; }
const it = g("k");
it.next();
const outcome = it.next().value;
console.log("computed", outcome.a, outcome.k, JSON.stringify(outcome));

function* g2(key: any) { let o = { [key]: 1, a: 'a' }; yield 20; return o; }
const it2 = g2(undefined);
console.log(it2.next().value);
const o2 = it2.next().value;
console.log("computed-undef", o2[undefined], o2["undefined"], o2.a);

function* g3() { const p = { z: 9 }; let o = { __proto__: p, a: 'a' }; yield 1; return o; }
const it3 = g3(); it3.next();
const o3 = it3.next().value;
console.log("proto", o3.a, o3.z, Object.getPrototypeOf(o3).z);
