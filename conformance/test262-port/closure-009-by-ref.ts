// Closure Copy-capture is now by-reference for non-escaping
// closures. Mutations inside the arrow body propagate back to the
// outer binding (matches JS-spec closure semantics; previously
// every mutation wrote to a local copy and was lost).
//
// Implementation: ssa_lower's Closure construction site detects
// non-escaping closures (the enclosing fn's return type is NOT a
// Closure) and stores the outer alloca's POINTER into env+offset
// for each Copy-typed capture. The closure body's __env decode
// loads the pointer and uses it directly as the capture's local
// slot, so loads / stores transparently flow through to the
// original outer binding. Zero heap allocation, zero leak.
//
// Escaping closures (the factory pattern: fn returning a closure)
// fall back to by-value capture for Copy types — the outer slot
// would dangle past the construction frame, and a proper fix needs
// env-drop machinery. Documented as
// project_closure_box_leak_followup. Doesn't matter for the
// non-escape callbacks below.

function check(): number {
  // (1) Single Copy capture, mutation propagates
  let x = 0;
  let inc = () => { x = x + 1; };
  inc();
  inc();
  inc();
  if (x !== 3) { throw "#1 inc 3 times"; }

  // (2) Outer mutation visible to closure
  let y = 100;
  let read_y = (): number => { return y; };
  if (read_y() !== 100) { throw "#2a"; }
  y = 200;
  if (read_y() !== 200) { throw "#2b mutation visible"; }

  // (3) Multiple Copy captures, each independent
  let a = 1;
  let b = 10;
  let bump = (): void => { a = a + 1; b = b + 10; };
  bump();
  bump();
  if (a !== 3) { throw "#3a"; }
  if (b !== 30) { throw "#3b"; }

  // (4) Conditional mutation
  let counter = 0;
  let cond_inc = (do_it: boolean): void => {
    if (do_it) { counter = counter + 1; }
  };
  cond_inc(true);
  cond_inc(false);
  cond_inc(true);
  cond_inc(true);
  if (counter !== 3) { throw "#4 conditional"; }

  // (5) Closure passed as a callback that mutates outer
  let total = 0;
  let add = (v: number): void => { total = total + v; };
  add(5);
  add(7);
  add(11);
  if (total !== 23) { throw "#5 add 5+7+11"; }

  return 0;
}
console.log(check());
