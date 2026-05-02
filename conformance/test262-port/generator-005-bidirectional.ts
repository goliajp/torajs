// Phase J.4 — bidirectional yield. `let v = yield e;` parses as
// Stmt::YieldInto, which desugar_generators expands to:
//   yield e;
//   let v = this.__sent;
// The iterator class gains a `__sent: <yield_ty>` field and `next()`
// takes an optional `__yield_arg: <yield_ty> = 0` parameter that is
// stashed into `this.__sent` on every resume. apply_default_args was
// extended to handle Member-shape calls (`obj.next()` over a
// generator) so callers can omit the arg without typecheck error.
//
// Limitation: yield types stay `number` only (the rest of J's MVP);
// the default-fill at no-arg call sites uses 0.

function* echoer(): number {
  let a = yield 1;
  let b = yield a + 10;
  yield b * 2;
}

function* sum_acc(): number {
  let total = 0;
  while (true) {
    let v = yield total;
    total = total + v;
  }
}

function* picky(threshold: number): number {
  while (true) {
    let v = yield 0;
    if (v >= threshold) {
      yield v;
    } else {
      yield -v;
    }
  }
}

function check(): number {
  // echoer — first call ignores arg, then send back values
  let g1 = echoer();
  if (g1.next().value !== 1) { throw "#1 echo init"; }
  if (g1.next(5).value !== 15) { throw "#2 echo a=5"; }   // a=5, yield a+10=15
  if (g1.next(7).value !== 14) { throw "#3 echo b=7"; }   // b=7, yield b*2=14
  if (g1.next().done !== true) { throw "#4 echo end"; }

  // sum_acc — accumulate values sent in
  let g2 = sum_acc();
  if (g2.next().value !== 0) { throw "#5 acc init"; }      // total=0
  if (g2.next(10).value !== 10) { throw "#6 acc 10"; }     // +10
  if (g2.next(25).value !== 35) { throw "#7 acc 25"; }     // +25
  if (g2.next(7).value !== 42) { throw "#8 acc 7"; }       // +7
  if (g2.next(0).value !== 42) { throw "#9 acc 0"; }       // +0

  // picky — branch on sent value
  let g3 = picky(10);
  if (g3.next().value !== 0) { throw "#10 picky init"; }
  if (g3.next(15).value !== 15) { throw "#11 picky 15"; }   // >= threshold → +
  if (g3.next().value !== 0) { throw "#12 picky 0 init"; }  // back to top
  if (g3.next(3).value !== -3) { throw "#13 picky 3"; }     // < threshold → -

  // Verify the older J.1 shape still works after apply_default_args's
  // Member-call extension (no-arg `g.next()` keeps J.1's behavior).
  // Note: when the generator binds via `let v = yield e;`, an arg
  // omission diverges between bun (undefined → NaN on arithmetic) and
  // tr (typed-zero default for the yield_ty), so this test only
  // exercises the no-bind shape.
  let g4 = sum_acc();
  if (g4.next().value !== 0) { throw "#14 noarg init"; }
  if (g4.next(3).value !== 3) { throw "#15 noarg 3"; }

  return 0;
}
console.log(check());
