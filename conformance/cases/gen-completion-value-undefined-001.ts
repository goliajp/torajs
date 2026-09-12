// RFC 20260805-generator-value-channels, the body → caller half: an
// exhausted generator's step is `{value: undefined, done: true}`
// (§27.5.1.2), `yield*` over it answers `undefined`, and a bare
// `return;` in a `number` generator compiles. The step's `value: T`
// slot is NOT widened (the yield path stays unboxed): a `number` slot
// carries the F64 undefined sentinel, and every consumer that tells
// `undefined` from a number — typeof, print, strict-eq, JSON, a read
// through an `any` receiver — recognises it. Before: the completion
// value was `T`'s zero (`0`), and `return;` was a type error.
function* g(): number { yield 1; }
const it = g(); it.next();
const s = it.next();
console.log("exhausted", s.value, typeof s.value, s.done, s.value === undefined, JSON.stringify(s));
function* d(): number { const r = yield* g(); console.log("delegate-ret", r, typeof r); }
const id = d(); id.next(); id.next();
function* e(): number { yield 1; return; }
const ie = e(); ie.next(); const se = ie.next(); console.log("bare-return", se.value, se.done);
function* f(): number { yield 1; return 7; }
const iff = f(); iff.next(); console.log("return-7", iff.next().value);
function* st(): string { yield "a"; }
const ist = st(); ist.next(); const ss = ist.next(); console.log("str-exhausted", ss.value, ss.done, ss.value === undefined);
function* an(): any { yield 1; }
const ian = an(); ian.next(); console.log("any-exhausted", ian.next().value);
let total = 0;
for (const v of g()) total = total + v;
console.log("forof-total", total);
const arr = [...g()]; console.log("spread", arr.length, arr[0]);
console.log("after-exhaust", it.next().value, it.next().done);
const anyIt: any = g(); anyIt.next();
const anyStep = anyIt.next();
console.log("any-recv", anyStep.value, typeof anyStep.value, anyStep.done);
