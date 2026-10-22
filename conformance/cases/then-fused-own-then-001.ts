// `await p.then(f)` in the module body over a pending cell: an own
// `then` stored on one cell is the call (§27.2.5.1 Invoke), the other
// cell runs the builtin.
function twice(v: number): number {
  return v * 2;
}

async function later(v: number): Promise<number> {
  await null;
  return v;
}

const p = later(4);
const q = later(5);
(q as any).then = function (f: (v: number) => number) {
  console.log("own then");
  return Promise.resolve(f(100));
};
console.log(await p.then(twice));
console.log(await q.then(twice));
