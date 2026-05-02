// Phase J.3 — `yield*` generator delegation. Parser desugars
// `yield* gen(args)` (where `gen` is a known function* declaration)
// into the iterator-protocol drain shape:
//
//   { let __it: __Gen_<gen> = gen(args);
//     while (true) {
//       let __step: __step_<gen> = __it.next();
//       if (__step.done) { break; }
//       yield __step.value;
//     } }
//
// The generated `yield` re-uses J.2.b's state-machine lowering, so
// every value the inner generator yields produces one resume point
// in the outer state machine. Multi-level chains compose naturally —
// each `yield*` desugars independently, all eventually feeding the
// outermost `for-of` consumer.
//
// Limitation (J.3 MVP): the source must be a direct `Ident(...)`
// call to a parser-tracked `function*`. Captured iterators
// (`let g = gen(); yield* g;`) need type-driven dispatch.

function* leaf(n: number): number {
  let i = 0;
  while (i < n) {
    yield i;
    i = i + 1;
  }
}

function* squares(n: number): number {
  for (let k = 0; k < n; k++) {
    yield k * k;
  }
}

function* prefixed(): number {
  yield 100;
  yield* leaf(3);  // expands to 0, 1, 2
  yield 200;
}

function* nested(): number {
  yield -1;
  yield* prefixed();  // expands to 100, 0, 1, 2, 200
  yield -2;
}

function* mixed(n: number): number {
  yield 0;
  yield* squares(n);
  yield -1;
  yield* leaf(n);
}

// Inner that yields nothing — exercises yield* over an empty sub-gen
function* empty_seq(): number {
  // intentional no-op body
  let _u = 0;
  if (_u > 0) { yield 0; }  // unreachable, parser still accepts the yield
}

function* with_empty(): number {
  yield 1;
  yield* empty_seq();
  yield 2;
}

function check(): number {
  // Basic delegation — sum 100 + (0+1+2) + 200 = 303
  let s1: number = 0;
  for (let v of prefixed()) {
    s1 = s1 + v;
  }
  if (s1 !== 303) { throw "#1 prefixed sum"; }

  // 3-level nesting — top wraps prefixed wraps leaf
  // Sequence: -1, 100, 0, 1, 2, 200, -2 → sum = 300
  let s2: number = 0;
  for (let v of nested()) {
    s2 = s2 + v;
  }
  if (s2 !== 300) { throw "#2 nested sum"; }

  // Two yield*s in one body, with literal yields between them
  // Sequence (n=4): 0, 0, 1, 4, 9, -1, 0, 1, 2, 3 → sum = 19
  let s3: number = 0;
  for (let v of mixed(4)) {
    s3 = s3 + v;
  }
  if (s3 !== 19) { throw "#3 mixed sum"; }

  // Order check via accumulator string-encoding
  // Drive prefixed() one yield at a time and verify exact sequence
  let g = prefixed();
  if (g.next().value !== 100) { throw "#4 prefixed[0]"; }
  if (g.next().value !== 0)   { throw "#5 prefixed[1]"; }
  if (g.next().value !== 1)   { throw "#6 prefixed[2]"; }
  if (g.next().value !== 2)   { throw "#7 prefixed[3]"; }
  if (g.next().value !== 200) { throw "#8 prefixed[4]"; }
  if (g.next().done !== true) { throw "#9 prefixed end"; }

  // Empty inner — yield* leaf(0) yields nothing; outer yields just
  // the surrounding values.
  let h = with_empty();
  if (h.next().value !== 1) { throw "#10 empty[0]"; }
  if (h.next().value !== 2) { throw "#11 empty[1]"; }
  if (h.next().done !== true) { throw "#12 empty end"; }

  return 0;
}
console.log(check());
