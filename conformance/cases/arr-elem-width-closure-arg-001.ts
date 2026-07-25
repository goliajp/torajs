// An array handed to an arrow bound to a local must stay in one width
// class with the parameter that receives it.
//
// A call whose callee names a known function joins each argument's
// container class onto the parameter's. A call through a function VALUE
// took a different path that passed along only the argument's scalar
// width, never the container join — and an arrow is lifted to a
// synthetic `__closure_N` before this analysis runs, so `bump(a)` below
// goes through that second path.
//
// The two ends then disagreed about the element width: the parameter's
// slot widened (the fractional write is right there in its body) while
// the caller's array stayed narrow. Writing 1.5 through the parameter
// put f64 bits into an i64-typed array, and reading it back gave
// 4609434218613702700 — silently, with the process exiting 0.
//
// The projection the value-call path already builds is glued to the
// lifted function's parameter key by `fn_value_flow` at the binding's
// init, so joining the argument onto that projection reaches it.

// write through an arrow bound to a local
const bump = (xs: number[]): void => {
  xs[0] = 1.5;
};
const a: number[] = [1, 2];
bump(a);
console.log(a[0], a[1]);

// the same through a binding aliased to the arrow
const bump2 = bump;
const b: number[] = [3, 4];
bump2(b);
console.log(b[0], b[1]);

// a named function was always right — regression guard for the path
// that already joined
function bumpNamed(xs: number[]): void {
  xs[0] = 2.5;
}
const c: number[] = [5, 6];
bumpNamed(c);
console.log(c[0], c[1]);

// reading back out through the parameter, same class
const first = (xs: number[]): number => xs[0];
console.log(first([10, 20]));
const d: number[] = [7, 8];
d[1] = 0.5;
console.log(first(d), d[1]);

// an all-integral class stays narrow across the same boundary
const idx0 = (xs: number[]): number => xs[0];
console.log(idx0([11, 12]));

// nested arrays cross the boundary too
const bumpGrid = (g: number[][]): void => {
  g[0][0] = 1.5;
};
const grid: number[][] = [[1, 2]];
bumpGrid(grid);
console.log(grid[0][0], grid[0][1]);
