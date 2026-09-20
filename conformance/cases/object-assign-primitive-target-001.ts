// ES §20.1.2.1 step 1 is ToObject(target). For a primitive that means
// the writes land on a WRAPPER, and the wrapper is what the call
// answers -- `typeof Object.assign(1, {})` is "object", not "number".
//
// The lowering has described this in a comment for a long time and has
// the kernel for it (anyv_to_object_strict), but that arm was only
// reachable when the target's static type was already `any`. The
// checker refused every typed primitive before it got there, which is
// why the same value spelled `any` worked and the typed spelling did
// not.
function t(f: () => void): string { try { f(); return "no-throw"; } catch (e) { return "threw"; } }

console.log(JSON.stringify(Object.assign(1, { a: 1 })));
console.log(JSON.stringify(Object.assign("x", { a: 1 })));
console.log(JSON.stringify(Object.assign(true, { a: 1 })));
console.log(typeof Object.assign(1, {}));
console.log(typeof Object.assign("x", {}));

// the wrapper really does take the write, it just is not observable
// through JSON -- read it back off the returned object
const w: any = Object.assign(1, { a: 7 });
console.log(w.a);

// the any spelling has always been right; the two must agree
const n: any = 1;
console.log(JSON.stringify(Object.assign(n, { a: 1 })));
console.log(typeof Object.assign(n, {}));

// nullish still throws through band B -- ToObject step 1
console.log(t(() => { Object.assign(null, { a: 1 }); }));
console.log(t(() => { Object.assign(undefined, { a: 1 }); }));

// object targets unchanged. A non-empty struct target still obeys the
// static subset rule (every source property needs a slot on the
// target) -- that rule is untouched here, so the source shape matches.
console.log(JSON.stringify(Object.assign({ z: 0 }, { z: 9 })));
console.log(JSON.stringify(Object.assign({}, { a: 1 }, { b: 2 })));
