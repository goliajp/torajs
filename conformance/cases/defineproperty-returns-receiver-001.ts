// P3.3.retval — `Object.defineProperty(O, k, desc)` returns O (ES §20.1.2.6).
// Pre-fix tr returned undefined, breaking common patterns like
// `let root = Object.defineProperty({}, ...)` and downstream
// `Object.create(root, ...)` which then threw with
// "Object prototype may only be an Object or null.".

const o: any = { seed: 7 };
const r: any = Object.defineProperty(o, 'x', { value: 1 });
console.log('typeof r:', typeof r);
console.log('r === o:', r === o);
console.log('r.seed:', r.seed);

// The test262 __lookupGetter__ chain pattern that motivated the fix —
// we care about `typeof root === 'object'` (pre-fix undefined) and
// that `Object.create(root, ...)` no longer throws.
const root: any = Object.defineProperty({ seed: 'S' }, 'x', { value: 42 });
console.log('typeof root:', typeof root);
const subject: any = Object.create(root, { y: { value: 99 } });
console.log('typeof subject:', typeof subject);
