// P5.1 — IteratorResult<T> structural alias for `{ value: T, done:
// boolean }`. Iterator<T> / IterableIterator<T> resolve to `any` so
// user code can annotate iterator-typed fields/returns without
// triggering "unresolved type" errors (the spec-shape methods are
// resolved through dynobj dispatch at P5.3 Phase B time).

function step(v: number, done: boolean): IteratorResult<number> {
  return { value: v, done: done };
}

const a = step(7, false);
console.log(a.value);  // 7
console.log(a.done);   // false

const eof = step(0, true);
console.log(eof.value); // 0
console.log(eof.done);  // true

// IteratorResult<string> with a string value.
function strStep(s: string, done: boolean): IteratorResult<string> {
  return { value: s, done: done };
}
const b = strStep("hi", false);
console.log(b.value);  // hi
console.log(b.done);   // false

// Iterator<T> annotation resolves to Any — used as an opaque handle
// without surfacing a typecheck error.
function getIter(): Iterator<number> { return ({} as any); }
const it = getIter();
console.log(typeof it); // object
