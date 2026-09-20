// ES §28.1.11 step 1 — `Reflect.ownKeys` is NOT a ToObject surface. It
// throws a TypeError on a non-Object target instead of wrapping it,
// which is exactly how the whole Reflect family differs from the
// Object.* one: Object.keys(1) is [], Reflect.ownKeys(1) throws.
//
// tr aliases the two onto one emit, and that part is right -- own
// string keys and all own keys coincide for the shapes that reach it.
// What the alias did not carry over is the IsObject gate, so every
// primitive target refused at compile time where the spec wants a
// catchable TypeError.
function t(f: () => void): string { try { f(); return "no-throw"; } catch (e) { return "threw"; } }

console.log(t(() => { Reflect.ownKeys(1); }));
console.log(t(() => { Reflect.ownKeys(true); }));
console.log(t(() => { Reflect.ownKeys("ab"); }));
console.log(t(() => { Reflect.ownKeys(1n); }));
console.log(t(() => { Reflect.ownKeys(Symbol("k")); }));
console.log(t(() => { Reflect.ownKeys(null); }));
console.log(t(() => { Reflect.ownKeys(undefined); }));

// the any spelling must agree with the typed one
const n: any = 1;
console.log(t(() => { Reflect.ownKeys(n); }));

// object targets keep working, and keep including length for an array
console.log(JSON.stringify(Reflect.ownKeys({ a: 1 })));
console.log(JSON.stringify(Reflect.ownKeys([1, 2])));

// the Object.* side of the alias is a ToObject surface and must NOT
// have moved: it still refuses a primitive today (that is the next
// knife), and it still answers for objects.
console.log(JSON.stringify(Object.keys({ a: 1 })));
console.log(JSON.stringify(Object.keys([1, 2])));
