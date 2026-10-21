// `.catch(onErr)` on a `Promise.reject(...)` mint: the source never
// fulfils, so the derived promise settles with onErr's value alone and a
// following handler's parameter is that value's type.

function rec(e: any): number {
  return -1;
}

Promise.reject(new Error("a")).catch((e) => 41).then((v) => console.log("num", v + 1, typeof v));
Promise.reject("x").catch((e) => "s").then((v) => console.log("str", v + "!", v.length));
Promise.reject("x").catch(rec).then((v) => console.log("named", v - 1, typeof v));
Promise.reject(new Error("a"))
  .catch((e) => 41)
  .then((v) => v * 2)
  .then((w) => console.log("chain", w, typeof w));
Promise.reject(0).catch((e) => undefined).then((v) => console.log("undef", v, v === undefined));

const p = Promise.reject(new Error("q")).catch((e) => 3);
p.then((v) => console.log("binding", v + 0.5));

async function main(): Promise<void> {
  const a = await Promise.reject(new Error("a")).catch((e) => 7);
  console.log("await num", a * 2, typeof a);
  const b = await Promise.reject(new Error("a")).catch((e) => "z");
  console.log("await str", b.length, typeof b);
  const c = await Promise.reject(1).catch((e) => undefined);
  console.log("await undef", c, c === undefined);
  const d = await Promise.reject(new Error("a"))
    .catch((e) => 7)
    .then((v) => v + 1);
  console.log("await chain", d * 2);
  const inner = async (): Promise<number> => await Promise.reject(2).catch((e) => 5);
  console.log("nested", (await inner()) + 1);
}
main();
