// Ownership pass — verify that previously broken patterns now work
// after the multi-branch CFG-aware moved tracking + transitive
// consume analysis. Each section reproduces a bug that was either
// silent-wrong-output or a typecheck error before the fix.

class Box {
  v: number;
  constructor() { this.v = 0; }
  set(x: number): void { this.v = x; }
}

class Holder {
  arr: number[];
  constructor(a: number[]) { this.arr = a; }
  first(): number { return this.arr[0]; }
  second(): number { return this.arr[1]; }
}

// (1) Multi-branch return of the same class instance — used to
// typecheck-error "cannot transfer".
function pickem(flag: number): Box {
  let f = new Box();
  if (flag === 1) {
    f.set(10);
    return f;
  }
  f.set(20);
  return f;
}

// (2) Multi-branch return through a helper that returns its arg —
// used to silently produce wrong output (0 instead of mutated).
function set_and_return(b: Box, x: number): Box {
  b.set(x);
  return b;
}
function pickem_via_helper(flag: number): Box {
  let f = new Box();
  if (flag === 1) {
    return set_and_return(f, 10);
  }
  return set_and_return(f, 20);
}

// (3) Array stored in a class field — used to double-free at
// scope close (caller's arr + class's arr field both freed).
function array_into_class(): number {
  let arr: number[] = [3, -2, 1, -4];
  let h = new Holder(arr);
  // Reading the field via methods stays alive; previously the second
  // method call after dropping arr would crash on the freed buffer.
  return h.first() + h.second();
}

// (4) Generator with an array parameter — chains the same bug
// through the generator factory, which forwards to __new_*.
function* yield_pair(values: number[]): number {
  yield values[0];
  yield values[1];
}
function gen_array_drain(): number {
  let arr: number[] = [11, 22, 33, 44];
  let g = yield_pair(arr);
  let total: number = 0;
  total = total + g.next().value;
  total = total + g.next().value;
  return total;
}

// (5) Early-return from a function while another local still owns —
// the ssa_lower return-expr ident sweep prevents the still-owned
// local from being freed before the return.
function early_or_compute(flag: number): Box {
  let f = new Box();
  f.set(7);
  if (flag === 1) {
    return f;
  }
  let g = new Box();
  g.set(99);
  return g;
}

function check(): number {
  if (pickem(1).v !== 10) { throw "#1a multi-branch return then=10"; }
  if (pickem(0).v !== 20) { throw "#1b multi-branch return else=20"; }

  if (pickem_via_helper(1).v !== 10) { throw "#2a helper then=10"; }
  if (pickem_via_helper(0).v !== 20) { throw "#2b helper else=20"; }

  if (array_into_class() !== 1) { throw "#3 array-in-class first+second"; }   // 3 + -2 = 1

  if (gen_array_drain() !== 33) { throw "#4 gen array drain"; }   // 11 + 22 = 33

  if (early_or_compute(1).v !== 7) { throw "#5a early-return f"; }
  if (early_or_compute(0).v !== 99) { throw "#5b later return g"; }

  return 0;
}
console.log(check());
