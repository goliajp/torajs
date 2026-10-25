// A `for (let o = …)` head binding whose update makes closures lives in
// per-iteration boxes. Leaving the loop by return, throw or a labelled
// break must release every value the loop held once the closures that
// captured them are gone: the head's first value included. Each value
// is watched through a WeakRef and read after the function returns, in
// a later job. The count of live values is what this pins; it relies
// on a value being released when its last reference goes, which the
// specification leaves to the implementation, hence the .expected file.
type Cell = { n: number };
const refs: WeakRef<Cell>[] = [];
function mk(n: number): Cell {
  const c = { n: n };
  refs.push(new WeakRef(c));
  return c;
}

function byReturn(): number {
  const fs: (() => number)[] = [];
  for (let o = mk(0); o.n < 5; fs.push(() => o.n), o = mk(o.n + 1)) if (o.n === 2) return fs.length;
  return -1;
}

function byThrow(): number {
  const fs: (() => number)[] = [];
  try {
    for (let o = mk(10); o.n < 15; fs.push(() => o.n), o = mk(o.n + 1)) if (o.n === 12) throw new Error("x");
  } catch (e) {
    return fs.length;
  }
  return -1;
}

function byLabel(): number {
  const fs: (() => number)[] = [];
  outer: for (let k = 0; k < 2; k++)
    for (let o = mk(20); o.n < 25; fs.push(() => o.n), o = mk(o.n + 1)) if (o.n === 21) break outer;
  return fs.length;
}

function byBreak(): number {
  const fs: (() => number)[] = [];
  for (let o = mk(30); o.n < 35; fs.push(() => o.n), o = mk(o.n + 1)) if (o.n === 33) break;
  return fs.length;
}

console.log(byReturn(), byThrow(), byLabel(), byBreak(), refs.length);
Promise.resolve().then(() => {
  let live = 0;
  for (const r of refs) if (r.deref() !== undefined) live++;
  console.log("live", live);
});
