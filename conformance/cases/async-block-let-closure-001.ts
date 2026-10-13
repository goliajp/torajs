// a let / const declared in a loop body that awaits keeps its pass's value in the closures made after it
async function forBody(): Promise<string> {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    let n = i * 10;
    await null;
    fs.push(() => n);
  }
  return fs.map((f) => f()).join(",");
}
async function whileBody(): Promise<string> {
  const fs: Array<() => number> = [];
  let i = 0;
  while (i < 3) {
    const n = i * 2;
    await null;
    fs.push(() => n);
    i++;
  }
  return fs.map((f) => f()).join(",");
}
async function doWhileBody(): Promise<string> {
  const fs: Array<() => number> = [];
  let i = 0;
  do {
    const n = i;
    await Promise.resolve();
    fs.push(() => n);
    i++;
  } while (i < 3);
  return fs.map((f) => f()).join(",");
}
async function forOfBody(): Promise<string> {
  const fs: Array<() => string> = [];
  for (const s of ["p", "q"]) {
    const t = s.toUpperCase();
    await null;
    fs.push(() => t);
  }
  return fs.map((f) => f()).join(",");
}
async function written(): Promise<string> {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    let n = i;
    await null;
    fs.push(() => n);
    n = n * 100;
  }
  return fs.map((f) => f()).join(",");
}
async function nestedBlock(): Promise<string> {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    {
      const n = i + 7;
      await null;
      fs.push(() => n);
    }
  }
  return fs.map((f) => f()).join(",");
}
(async () => {
  console.log(await forBody());
  console.log(await whileBody());
  console.log(await doWhileBody());
  console.log(await forOfBody());
  console.log(await written());
  console.log(await nestedBlock());
})();
