// Generic async functions (one reading `arguments`), a private async
// method reading `arguments`, an async method of a generic class and a
// generic async declaration nested in a synchronous body suspend at their
// awaits against the job queue.
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) {
    p = p.then(() => {
      log.push(tag + i);
    });
  }
  return p;
}
async function g<T>(t: T): Promise<T> {
  const v = await Promise.resolve(t);
  log.push("g-after");
  return v;
}
async function ga<T>(t: T, u: T) {
  const v = await t;
  log.push("ga " + arguments.length);
  return [v, u];
}
class P {
  #k = 3;
  async #m(x: number) {
    await null;
    log.push("m " + arguments.length);
    return x + this.#k;
  }
  run() {
    return this.#m(4);
  }
}
class Box<T> {
  constructor(public v: T) {}
  async get(): Promise<T> {
    await null;
    log.push("get");
    return this.v;
  }
}
function outer(n: number) {
  async function id<X>(x: X): Promise<X> {
    await x;
    log.push("id");
    return x;
  }
  return id(n);
}
async function main() {
  ticks("A", 5);
  log.push("g " + (await g(5)) + " " + (await g("s")));
  ticks("B", 5);
  log.push("ga " + (await ga(1, 2)).join(","));
  ticks("C", 5);
  log.push("P " + (await new P().run()));
  ticks("D", 5);
  log.push("box " + (await new Box(2).get()));
  ticks("E", 5);
  log.push("outer " + (await outer(9)));
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
