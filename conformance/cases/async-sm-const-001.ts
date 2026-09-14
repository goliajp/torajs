// RFC 20260915 #8 — `const` / `using` bindings lifted into the async
// machine keep working as bindings: readable across awaits, block-
// scoped shadows, the for-of binding, and a nested async value that
// reads the frame. The immutability itself is a compile-time
// rejection (`x = 2` / `x++` / a nested write), the same message the
// sync lane gives — probed outside this suite, since bun and tsc
// reject those programs too.
async function sum(xs: number[]): Promise<number> {
  const base = 10;
  let acc = 0;
  for (const x of xs) {
    await null;
    acc += x + base;
  }
  {
    const base = acc * 2;
    await null;
    acc = base;
  }
  const tail = async (): Promise<number> => {
    await null;
    return base + acc;
  };
  return await tail();
}
async function disposal(): Promise<string> {
  const log: string[] = [];
  {
    await using r = { [Symbol.asyncDispose]() { log.push("disposed"); return Promise.resolve(0); } } as any;
    await null;
    log.push("body");
  }
  const joined = log.join(",");
  await null;
  return joined;
}
sum([1, 2, 3]).then((v) => console.log(v));
disposal().then((s) => console.log(s));
