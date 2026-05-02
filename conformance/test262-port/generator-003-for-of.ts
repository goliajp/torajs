// Phase I.2 — `for (let v of <generator-call>)` over user iterables.
// Parser tracks `function*` declarations; for-of with a direct call
// to one emits the iterator-protocol shape:
//
//   { let __it = gen(args);
//     while (true) {
//       let __step = __it.next();
//       if (__step.done) { break; }
//       let v = __step.value;
//       <body>
//     } }
//
// `break` / `continue` inside the body bind to the inner while-true,
// matching JS for-of semantics (break ends iteration, continue calls
// next() again).
//
// Limitation: only direct call sites are detected — `let g = gen();
// for (let v of g)` falls back to the array path and fails to type-
// check (no `.length` on the iterator class).

function* count_up_to(n: number): number {
  let i = 0;
  while (i < n) {
    yield i;
    i = i + 1;
  }
}

function* range_for(n: number): number {
  for (let k = 0; k < n; k++) {
    yield k;
  }
}

function* odds_below(n: number): number {
  for (let p = 0; p < n; p++) {
    if (p % 2 === 0) { continue; }
    yield p;
  }
}

function check(): number {
  // Basic drain — sum first 5 ints
  let sum1: number = 0;
  for (let v of count_up_to(5)) {
    sum1 = sum1 + v;
  }
  if (sum1 !== 10) { throw "#1 basic drain"; }  // 0+1+2+3+4

  // Empty range — body runs zero times
  let touched: number = 0;
  for (let v of range_for(0)) {
    touched = touched + 1;
  }
  if (touched !== 0) { throw "#2 empty"; }

  // break out of the for-of (early exit before exhaustion)
  let sum2: number = 0;
  for (let v of count_up_to(10)) {
    if (v >= 4) { break; }
    sum2 = sum2 + v;
  }
  if (sum2 !== 6) { throw "#3 break"; }  // 0+1+2+3

  // continue — skip even values from a 0..6 sequence
  let sum3: number = 0;
  for (let v of count_up_to(6)) {
    if (v % 2 === 0) { continue; }
    sum3 = sum3 + v;
  }
  if (sum3 !== 9) { throw "#4 continue"; }  // 1+3+5

  // for-of over a generator that itself uses control flow internally
  let sum4: number = 0;
  for (let v of odds_below(10)) {
    sum4 = sum4 + v;
  }
  if (sum4 !== 25) { throw "#5 odds"; }  // 1+3+5+7+9

  // Nested for-of — both layers run iterator-protocol while-loops
  let sum5: number = 0;
  for (let a of count_up_to(3)) {
    for (let b of count_up_to(3)) {
      sum5 = sum5 + a * 10 + b;
    }
  }
  if (sum5 !== 99) { throw "#6 nested"; }

  return 0;
}
console.log(check());
