// `await p.then(f)` in the module body where no cell can carry an own
// `then`: the reaction runs in place, after the jobs queued ahead of it.
function inc(v: number): number {
  console.log("inc", v);
  return v + 1;
}

async function later(v: number): Promise<number> {
  await null;
  return v;
}

let total = 0;
for (let i = 0; i < 3; i = i + 1) {
  const p = later(i);
  Promise.resolve(i).then((x: number) => console.log("queued", x));
  total = total + (await p.then(inc));
}
console.log("total", total);
