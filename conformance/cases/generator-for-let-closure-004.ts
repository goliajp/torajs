// for-let closures made in a loop head, with `continue` in the body, in a generator and an async function
function* g() {
  const fs: Array<() => number> = [];
  outer: for (let i = 0; i < 3; fs.push(() => i * 10), i++) {
    for (let j = 0; j < 3; fs.push(() => i * 100 + j), j++) {
      if (j === 1) continue;
      if (i === 1) continue outer;
      yield i * 10 + j;
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g()].join(" "));
async function a() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; fs.push(() => i), i++) {
    if (i === 1) continue;
    await null;
  }
  return fs.map((f) => f()).join(",");
}
a().then(console.log);
