// cm_demote As-shaped receivers — `(expr as any).next()` while some
// class (here a generator's __Gen_*) owns a `next` method: the
// speculative name-keyed `__cm_<C>__next(recv)` rewrite must demote
// back to the runtime any-dispatch instead of rejecting at
// "expected ClassRef, got Any" (S2.34 admitted Call receivers; the
// As shape was missed).
function* g() {
  yield 10;
  yield 20;
}
const it = ([1, 2].values() as any).next();
console.log(it.value, it.done);
console.log((g() as any).next().value);
const m = new Map([["k", 7]]);
console.log((m.entries() as any).next().value[1]);
