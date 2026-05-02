// Escape closures (factory pattern: fn returning a closure) with
// mutating Copy captures now propagate mutation back to the boxed
// outer slot. The let-decl pre-pass detects names captured by
// closures inside fns whose declared return type is a Closure
// type, and heap-allocates those slots so the env can hold a
// stable pointer that outlives the construction frame. Both outer
// and closure-body reads/writes flow through the same heap cell.
//
// Edge: the slot leaks until env-drop machinery lands (small, one
// 8-byte alloc per escape-captured Copy local). project_closure_
// box_leak_followup tracks the residual.

function makeCounter(): () => number {
  let n = 0;
  return (): number => {
    n = n + 1;
    return n;
  };
}

function makeAdder(seed: number): (delta: number) => number {
  // `seed` is a param captured by the returned closure; the same
  // heap-promotion path applies to params as to lets.
  return (delta: number): number => {
    seed = seed + delta;
    return seed;
  };
}

function makeReader(): () => number {
  let v = 100;
  return (): number => {
    return v;
  };
}

function check(): number {
  // (1) Counter — mutation persists across calls
  let c = makeCounter();
  if (c() !== 1) { throw "#1a"; }
  if (c() !== 2) { throw "#1b"; }
  if (c() !== 3) { throw "#1c"; }

  // (2) Independent counters — each captures its own boxed slot
  let d = makeCounter();
  if (d() !== 1) { throw "#2a independent"; }
  if (c() !== 4) { throw "#2b c-still-mine"; }
  if (d() !== 2) { throw "#2c d-still-mine"; }

  // (3) Adder with captured param — same heap-promotion for params
  let add = makeAdder(10);
  if (add(5) !== 15) { throw "#3a"; }
  if (add(3) !== 18) { throw "#3b"; }
  if (add(-8) !== 10) { throw "#3c"; }

  // (4) Independent adders
  let add2 = makeAdder(100);
  if (add2(1) !== 101) { throw "#4a independent"; }
  if (add(0) !== 10) { throw "#4b add-still-mine"; }

  // (5) Read-only escape closure — by-ref still correct
  let r = makeReader();
  if (r() !== 100) { throw "#5a"; }
  if (r() !== 100) { throw "#5b"; }

  return 0;
}
console.log(check());
