// 11-A1 deque-escape: aliases behind value-transparent wrappers
// (ternary / nullish / `as` / member-store of a ternary) must mark
// the source array deque-unsafe — a bare-Ident-only escape check
// read the pre-shift slot via the head-free fast path.
function ternaryAlias(): void {
  const xs = [10, 20, 30];
  const ys = [1, 2, 3];
  const y = true ? xs : ys;
  y.shift();
  console.log(xs[0]);
  console.log(xs.length);
}
ternaryAlias();

function memberStoreTernary(): void {
  const xs = [10, 20, 30];
  const box = { a: [0] };
  box.a = true ? xs : xs;
  box.a.shift();
  console.log(xs[0]);
}
memberStoreTernary();

function asCastAlias(): void {
  const xs = [10, 20, 30];
  const y = xs as number[];
  y.shift();
  console.log(xs[0]);
}
asCastAlias();

function assignTernaryAlias(): void {
  const xs = [10, 20, 30];
  const ys = [1, 2, 3];
  let y = ys;
  y = false ? ys : xs;
  y.shift();
  console.log(xs[0]);
}
assignTernaryAlias();
