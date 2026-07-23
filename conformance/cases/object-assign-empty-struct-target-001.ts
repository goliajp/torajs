// `Object.assign({}, ...sources)` — empty struct-literal target.
// Pre-fix: the checker's subset gate rejected any source prop because
// the target `{}` has no slot for it; the empty struct branch is now
// routed to the runtime any-lane, backed by a fresh dynobj alloc.
const r1: any = Object.assign({}, { a: 1 });
console.log(r1.a);

const r2: any = Object.assign({}, { a: 1 }, { b: 2 });
console.log(r2.a, r2.b);

// Last-source-wins on repeated key.
const r3: any = Object.assign({}, { a: 1 }, { a: 9 });
console.log(r3.a);

// Zero sources — the target passes through as an empty Any object.
const r4: any = Object.assign({});
console.log(Object.keys(r4).length);

// Nullish sources skip (§20.1.2.1 step 4.a); non-nullish still land.
const r5: any = Object.assign({}, null, { x: 1 }, undefined, { y: 2 });
console.log(r5.x, r5.y);

// Chained assign — the returned value is a real live Any object.
const r7: any = Object.assign({}, { a: 1 });
r7.b = 2;
console.log(r7.a, r7.b);
