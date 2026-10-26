// A class declared in an async generator's body evaluates its computed
// keys where it stands, in order (§15.7.14), and suspends at an await in
// one of them.

const log: string[] = [];
class Base {
  b() { return "base" }
}
async function* g() {
  class C extends Base {
    [(log.push("k1"), "x")]() { return 1 }
    [await Promise.resolve("y")]() { return 10 }
    static [(log.push("k3"), "z")]() { return 100 }
  }
  const c: any = new C();
  log.push("c" + c["x"]() + c["y"]() + (C as any)["z"]() + c.b());
  yield 1;
}
// Each key is converted to a property key right after it is evaluated
// (§13.2.5.4), so a key object's `toString` runs before the next key's
// await, not when the class is made; a symbol key is its own key.
function key(n: string): any {
  return { toString() { log.push("ts" + n); return n } };
}
async function* h() {
  class D {
    [key("a")]() { return 1 }
    [Symbol.toPrimitive]() { return 78 }
    [await Promise.resolve(key("b"))]() { return 2 }
    [key("c")]() { return 3 }
    static [(log.push("s"), "d")] = 4;
    [`e${await Promise.resolve(5)}`]() { return 5 }
  }
  const d: any = new D();
  log.push("r" + d.a() + d.b() + d.c() + (D as any).d + d.e5() + d[Symbol.toPrimitive]());
  yield 2;
}
async function main() {
  Promise.resolve().then(() => log.push("t0")).then(() => log.push("t1")).then(() => log.push("t2"));
  for await (const v of g()) log.push("v" + v);
  console.log(log.join(" "));
  log.length = 0;
  Promise.resolve().then(() => log.push("t0")).then(() => log.push("t1")).then(() => log.push("t2"));
  for await (const v of h()) log.push("v" + v);
  console.log(log.join(" "));
}
main();
