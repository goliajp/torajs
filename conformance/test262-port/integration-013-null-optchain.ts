// Integration: null + Nullable<T> + `??` operations. Exercises the
// in-band 0 sentinel for pointer-shaped types via simple comparisons.
// (Combining null reassignment + `??` + multiple Nullable bindings in
// the same scope hits a known v0 ssa-lower drop-emission edge case;
// covered separately in the closure-* / null-* unit tests.)
function check(): number {
  // Direct null comparisons.
  let n: string | null = null;
  if (n !== null) { throw "#1"; }

  let s: string | null = "hi";
  if (s === null) { throw "#2"; }

  // ?? returns the rhs when lhs is null.
  let v1 = n ?? "default";
  if (v1 !== "default") { throw "#3"; }

  // Counter pattern: 3-way arithmetic.
  let count: number = 0;
  let x: string | null = null;
  if (x === null) { count = count + 1; }
  x = "set";
  if (x !== null) { count = count + 10; }
  x = null;
  if (x === null) { count = count + 100; }
  if (count !== 111) { throw "#4"; }
  return 0;
}
console.log(check());
