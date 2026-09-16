// The stepping lane inside a generator: the park slot is lifted to a
// state-struct field, and a yield in an element's target suspends
// between that element's lref and its step. Stopping short of a
// generator source still runs its finally through IteratorClose.
function* src(): any {
  try {
    yield 1;
    yield 2;
    yield 3;
  } finally {
    console.log("src finally");
  }
}
const o: any = {};
[o["a"]] = src();
console.log("stopped at", o.a);
function* h(): any {
  const r: any = {};
  [r[yield "ask"], r["z"]] = src();
  console.log("in gen", r.one, r.z);
}
const it = h();
console.log(it.next().value);
it.next("one");
// the for-of head twin of the same desugar
const p: any = {};
for ([p["x"], p["y"]] of [
  [1, 2],
  [3, 4],
]) {
  console.log(p.x, p.y);
}
