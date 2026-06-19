// S200 — typed Map/Set has/get/delete accept 0-arg per ES default-undefined.
// Spec refs: §24.1.3.{3,4,6} Map.prototype.{delete,has,get},
//            §24.2.3.{4,7}   Set.prototype.{delete,has}.
// Step 1 of each fills the missing arg with undefined; typed Map<K,V> with
// K≠Any and Set<T> with T≠Any can't store an undefined key/element so the
// lookup always misses → undefined / false / false respectively.
// Pre-S200 tr panicked "index out of bounds" in ssa_lower because the
// `args[0]` access ran without an arity short-circuit. check.rs already
// permits 0-arg; only the lower needed a default-fill arm.

// Map<string, number>. Avoid Array.from(MapIter) in the post-check —
// that path is a separate L3b (typed Array<T>.entries widen + MapIter
// → Array<Any>).
{
  const m = new Map<string, number>([['a', 1], ['b', 2]]);
  console.log('typed-map-has-no-arg:', m.has());
  console.log('typed-map-get-no-arg:', m.get());
  console.log('typed-map-delete-no-arg:', m.delete());
  console.log('typed-map-after-size:', m.size);
  console.log('typed-map-after-existing-has:', m.has('a'));
  console.log('typed-map-after-existing-get:', m.get('a'));
}

// Map<number, string> (key flavor mismatch — same default rule).
{
  const m = new Map<number, string>([[1, 'one'], [2, 'two']]);
  console.log('numkey-map-has:', m.has());
  console.log('numkey-map-get:', m.get());
  console.log('numkey-map-delete:', m.delete());
  console.log('numkey-map-after-size:', m.size);
}

// Set<number>.
{
  const s = new Set<number>([1, 2, 3]);
  console.log('typed-set-has-no-arg:', s.has());
  console.log('typed-set-delete-no-arg:', s.delete());
  console.log('typed-set-after-size:', s.size);
}

// Set<string>.
{
  const s = new Set<string>(['a', 'b']);
  console.log('strset-has:', s.has());
  console.log('strset-delete:', s.delete());
  console.log('strset-after-size:', s.size);
}

// Empty containers — the default-undefined miss must still produce a
// well-formed false / undefined rather than a runtime panic.
{
  const m = new Map<string, number>();
  const s = new Set<number>();
  console.log('empty-map-has:', m.has());
  console.log('empty-map-get:', m.get());
  console.log('empty-set-has:', s.has());
  console.log('empty-set-delete:', s.delete());
}

// Side-effect ordering — receiver evaluated even on the 0-arg short
// circuit (parity with bun, which still touches the binding to dispatch
// the method).
{
  let n = 0;
  const make = (): Map<string, number> => { n++; return new Map<string, number>(); };
  console.log('side-effect-map-has:', make().has());
  console.log('side-effect-counter:', n);
}
