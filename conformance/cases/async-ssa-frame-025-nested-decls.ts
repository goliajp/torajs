// An async function declared in a synchronous body suspends at its awaits
// like a top-level one: lifted out (recursive, called above its
// declaration), turned into a value because it captures, in a block at the
// top level, in an arrow's body and in a constructor, and two of one name in
// different bodies, one of them with a `for await`.
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) p = p.then(() => { log.push(tag + i); });
  return p;
}

// lifted (captures nothing), recursive, called before its declaration
function r2() {
  return fact(4);
  async function fact(n: number): Promise<number> {
    log.push("fact" + n);
    return n <= 1 ? 1 : n * (await fact(n - 1));
  }
}

// capturing: becomes a function value
function counter(start: number) {
  let n = start;
  async function bump(by: number) {
    await null;
    n += by;
    log.push("bump" + n);
    return n;
  }
  return bump;
}

// in a block at the top level
{
  async function blockFn(x: number) {
    const v = await Promise.resolve(x * 2);
    log.push("block" + v);
    return v;
  }
  (globalThis as any).blockFn = blockFn;
}

// inside an arrow body and a class constructor
const viaArrow = (k: number) => {
  async function inner() {
    await 0;
    log.push("arrow" + k);
    return k;
  }
  return inner();
};
class Holder {
  p: Promise<string>;
  constructor(tag: string) {
    async function make() {
      await undefined;
      log.push("ctor" + tag);
      return tag + "!";
    }
    this.p = make();
  }
}

// same name in two bodies, one with a for await
function s1() {
  async function same(v: number) {
    await v;
    log.push("s1:" + v);
    return v;
  }
  return same(1);
}
function s2() {
  async function same(v: number) {
    for await (const x of [v, Promise.resolve(v + 1)]) log.push("s2:" + x);
    return v;
  }
  return same(5);
}

async function main() {
  ticks("A", 8);
  log.push("r2 " + (await r2()));
  ticks("B", 6);
  const b = counter(10);
  log.push("bump " + (await b(1)) + " " + (await b(2)));
  ticks("C", 6);
  log.push("block " + (await (globalThis as any).blockFn(3)));
  ticks("D", 6);
  log.push("arrow " + (await viaArrow(7)));
  ticks("E", 6);
  log.push("ctor " + (await new Holder("h").p));
  ticks("F", 8);
  log.push("same " + (await s1()) + " " + (await s2()));
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
