// An `async function*` declared in an async body becomes an async
// generator EXPRESSION bound in its slot (`desugar_async_nested_fns`):
// as a declaration it had no capture lane — `lift_nested_generators`
// links a sync generator's captures, and an async one reading an
// enclosing local was an unknown identifier — while the expression form
// takes the frame link the hoist gives an async value's nested
// generator.
async function decl() {
  const log: string[] = [];
  async function* gen() { log.push("in"); const x: any = yield 1; log.push("got:" + x); yield 2; }
  const it = gen();
  await it.next(); await it.next("A"); await it.next();
  console.log("decl", log.join(","));
}
function helper(fn: any) { return fn(); }
async function main() {
  await decl();
  await helper(async function () {
    let n = 0;
    async function* counter() { while (n < 3) { n += 1; yield n; } }
    const seen: number[] = [];
    for await (const v of counter()) seen.push(v);
    console.log("value", seen.join(","), n);
  });
  const outer: string[] = [];
  function* syncGen() { outer.push("sync"); yield 0; }
  for (const _ of syncGen()) {}
  console.log("sync", outer.join(","));
}
main();
