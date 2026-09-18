// An alias binding the body reassigns (`let b = xs[0]; … b = xs[j]`) is
// born owned. As a borrow, a path with NO assignment still reached a scope
// drop the borrow never paid for: the container's element was freed under
// it and the next same-size allocation read back through it.
function run(i: number): void {
  const xs = [{ s: "first" + i }, { s: "second" + i }];
  { let b = xs[0]; if (i < 0) b = xs[1]; }
  const t = { s: "later" + i };
  console.log(xs[0].s, t.s);
  const o = { inner: { v: "in" + i }, other: { v: "ot" + i } };
  { let m = o.inner; if (i < 0) m = o.other; }
  const u = { v: "new" + i };
  console.log(o.inner.v, u.v);
  let best = xs[0]; for (let j = 1; j < xs.length; j++) { if (xs[j].s.length > best.s.length) best = xs[j]; }
  console.log(best.s, xs[0].s, xs[1].s);
}
for (let i = 1; i <= 3; i++) run(i);
