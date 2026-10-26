// A `return` in a method of a class declared in an async generator's
// body is the method's own: it does not await its operand the way the
// generator's own `return` does (§27.6.3.2), so calling the method runs
// no job ahead of its turn.

const log: string[] = [];
async function* g() {
  class C {
    m() { return 10 }
    static s() { return 20 }
    [("k" + 1)]() { return 30 }
  }
  const c: any = new C();
  Promise.resolve().then(() => log.push("t0"));
  log.push("m" + c.m() + " s" + C.s() + " k" + c.k1());
  log.push("after");
  yield 1;
  return 2;
}
async function main() {
  const it = g();
  log.push(JSON.stringify(await it.next()));
  log.push(JSON.stringify(await it.next()));
  console.log(log.join(" "));
}
main();
