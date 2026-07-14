// RFC 20260714-dstr-residual blade 3 prereq — the unified runtime
// iteration protocol (`__torajs_any_iter_next`) over a Map / Set held
// behind `any`. Before this lane the kernel only knew the collections'
// ITERATOR cells (`m.keys()` / `m.entries()`), so the collection
// itself — the shape every `for (x of set)` actually writes — fell
// through to "value is not iterable".
//
// ES §24.2.5.1: a Set's default iterator is `values()` (identical to
// `keys()` — the storage parks `undefined` in every value slot).
// ES §23.1.4: a Map's is `entries()`, so the loop var is a `[k, v]`
// pair.

const s: any = new Set([1, 2, 3]);
for (const x of s) {
  console.log(x);
}

// Spread — the other `any_iter_next` driver — drains the same lane.
const drained: any[] = [...s];
console.log(drained.length, drained[0], drained[2]);

const m: any = new Map([["a", 1], ["b", 2]]);
for (const e of m) {
  console.log(e[0], e[1]);
}

// The derived iterator is parked per-loop, so a second walk over the
// same collection starts from the top rather than resuming a spent
// cursor.
for (const x of s) {
  console.log(x * 10);
}

// `break` leaves the loop through the same exit block that releases
// the derived iterator.
for (const x of s) {
  if (x === 2) {
    break;
  }
  console.log("kept", x);
}
