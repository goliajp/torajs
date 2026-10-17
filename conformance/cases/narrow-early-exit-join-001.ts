// What follows an `if` is narrowed when every branch that falls
// through leaves the binding non-null: `if (a === null || ...) return`
// (the `||` is false only when both sides are) and `if (a === null)
// a = new A()` (the then-branch fills it, the false side already was
// one). Both were refused at the later use.

class Wide {
  w: number;
  constructor(w: number) {
    this.w = w;
  }
}

function make(n: number): Wide | null {
  return n > 0 ? new Wide(n) : null;
}

function twice(x: Wide): number {
  return x.w * 2;
}

function small(n: number): number {
  const a = make(n);
  if (a === null || a.w < 2) return -1;
  return twice(a);
}
console.log("or-exit:", small(0), small(1), small(3));

function filled(n: number): number {
  let a = make(n);
  if (a === null) a = new Wide(9);
  return twice(a);
}
console.log("fill:", filled(0), filled(4));

function filledBlock(n: number): number {
  let a: Wide | null = make(n);
  if (a == null) {
    console.log("filling");
    a = new Wide(7);
  }
  return twice(a);
}
console.log("fill-block:", filledBlock(0), filledBlock(2));

function both(n: number, m: number): number {
  const a = make(n);
  if (m > 0 && a !== null) return twice(a) + m;
  return 0;
}
console.log("and:", both(1, 2), both(0, 2), both(1, 0));

function elseFill(n: number): number {
  let a = make(n);
  if (a !== null) {
    console.log("had", a.w);
  } else {
    a = new Wide(5);
  }
  return twice(a);
}
console.log("else-fill:", elseFill(0), elseFill(6));
