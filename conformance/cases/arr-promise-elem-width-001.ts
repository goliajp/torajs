// An array handed to `Promise.resolve` must stay in one width class
// with whatever awaits it.
//
// Which point the argument contributes depends on what it IS. Handing
// `resolve` another promise passes that promise's value through, so the
// two value points join. Handing it a plain container makes that
// container the value, so the argument's own point is what joins.
//
// Only the first reading existed. An array literal in
// `Promise.resolve([1, 2, 3])` therefore sat in a different class from
// the binding that awaited it: the binding widened on a fractional
// write while the literal stayed narrow, and reading it back gave
// 1e-323 — silently, with the process exiting 0.

async function writeThrough(): Promise<void> {
  const p = Promise.resolve([1, 2, 3]);
  const xs: number[] = await p;
  xs[0] = 1.5;
  console.log(xs[0], xs[1], xs[2]);
}
await writeThrough();

// awaiting the call directly, without the intermediate binding
async function direct(): Promise<void> {
  const ys: number[] = await Promise.resolve([4, 5, 6]);
  ys[1] = 0.5;
  console.log(ys[0], ys[1], ys[2]);
}
await direct();

// a promise handed to resolve still passes its value through — the
// reading that was already right
async function nested(): Promise<void> {
  const inner = Promise.resolve([7, 8]);
  const zs: number[] = await Promise.resolve(inner);
  zs[0] = 2.5;
  console.log(zs[0], zs[1]);
}
await nested();

// an all-integral class stays narrow across the same boundary
async function narrow(): Promise<void> {
  const ws: number[] = await Promise.resolve([9, 10]);
  console.log(ws[0], ws[1]);
}
await narrow();

// scalars through resolve are untouched
console.log(await Promise.resolve(42));
console.log(await Promise.resolve(2.5));
