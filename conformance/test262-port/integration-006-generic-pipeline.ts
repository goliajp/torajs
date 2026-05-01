// Integration: generic functions composed in a pipeline. Exercises
// monomorphization at multiple call sites + closure passing + array
// operations.
function id<T>(x: T): T { return x; }

function pair_of<T>(x: T): T[] {
  let r: T[] = [];
  r.push(x);
  r.push(x);
  return r;
}

function fst<T>(xs: T[]): T { return xs[0]; }

function check(): number {
  // Identity over multiple types.
  if (id(42) !== 42) { throw "#1"; }
  if (id("hi") !== "hi") { throw "#2"; }
  if (id(true) !== true) { throw "#3"; }

  // Pair-of returns 2-element array.
  let p1 = pair_of(7);
  if (p1.length !== 2) { throw "#4"; }
  if (p1[0] !== 7) { throw "#5"; }
  if (p1[1] !== 7) { throw "#6"; }

  let p2 = pair_of("a");
  if (p2.length !== 2) { throw "#7"; }
  if (p2[0] !== "a") { throw "#8"; }

  // First of pair.
  if (fst(p1) !== 7) { throw "#9"; }
  if (fst(p2) !== "a") { throw "#10"; }

  // Compose: id(fst(pair_of(x))) == x
  if (id(fst(pair_of(99))) !== 99) { throw "#11"; }
  return 0;
}
console.log(check());
