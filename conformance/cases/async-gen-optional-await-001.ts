// An await in the part of an optional chain a nullish base skips, in an
// async generator: `o?.[await k]`, `f?.(await x)`, `o.m?.(await x)`,
// `o?.m(await x)`. The base is evaluated once and tested; a nullish one
// is the whole chain's undefined with the key and the arguments never
// evaluated (§13.3.9.1), and a method call keeps its receiver.

const log: string[] = [];
function ticks(n: number) {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) p = p.then(() => { log.push("t" + i) });
}
function key(): Promise<string> {
  log.push("key");
  return Promise.resolve("p");
}
function arg(): Promise<number> {
  log.push("arg");
  return Promise.resolve(5);
}

async function* parts(o: any, n: any, f: any) {
  const a = o?.[await key()];
  log.push("a" + a);
  yield a;
  const b = n?.[await key()];
  log.push("b" + b);
  yield b;
  const c = f?.(await Promise.resolve(3));
  log.push("c" + c);
  yield c;
  const d = n?.(await key());
  log.push("d" + d);
  yield d;
  const e = o.m?.(await Promise.resolve(4));
  log.push("e" + e);
  yield e;
  const g = o.none?.(await key());
  log.push("g" + g);
  yield g;
}

async function* calls(o: any, n: any) {
  yield o?.m(await arg());
  yield n?.m(await arg());
  yield (await Promise.resolve(o))?.m(await arg());
  yield o?.["m"](await arg());
  yield n?.["m"](await arg());
  yield o?.["m"]?.(await arg());
  yield n?.m?.(await arg());
}

// A callee read once and called on its receiver (`o.m?.(x)`, a getter
// counting its reads), a chain going on past its optional link
// (`o?.a.m(x)`, `n?.p[await k].q`), a parenthesized chain (`(n?.m)(x)`,
// a TypeError after the argument), a typed nullable receiver.
let reads = 0;
class R {
  q = 9;
  get m() {
    reads++;
    log.push("get" + reads);
    const self = this;
    return function (this: any, v: number) { return this === self ? "recv" + v : "lost" + v };
  }
  n(v: number) { return this.q + v }
}
async function* chains(o: any, n: any, r: R | null, rn: R | null) {
  try { yield (n?.m)(await arg()); } catch (e: any) { log.push("te" + (e instanceof TypeError)); }
  yield (o.p?.m)(await arg());
  yield o.r.m?.(await arg());
  yield o?.r.m?.(await arg());
  yield o.r["m"]?.(await arg());
  yield o?.a.m(await arg());
  yield n?.a.m(await arg());
  yield n?.p[await key()];
  yield o?.p[(await key(), "m")](3);
  yield n?.p[await key()].q;
  yield r?.n(await arg());
  yield rn?.n(await arg());
  yield r?.q;
}

async function main() {
  const o = { p: 1, k: 10, m(x: number) { return this.k + x } };
  ticks(8);
  for await (const v of parts(o, null, (x: number) => x * 2)) log.push("v" + v);
  log.push("|");
  ticks(4);
  for await (const v of calls(o, undefined)) log.push("v" + v);
  console.log(log.join(" "));
  log.length = 0;
  const c: any = {
    p: { m(v: number) { return this.q + v }, q: 5 },
    a: { m(v: number) { return this.q + v }, q: 7 },
    r: new R(),
  };
  ticks(3);
  for await (const v of chains(c, null, new R(), null)) log.push("v" + v);
  console.log(log.join(" "));
}
main();
