// An await in an object literal's computed key suspends where the key is
// evaluated, just before its value (§13.2.5.5): in an async function,
// whose frame the key's await is one of the function's own, and in an
// async generator.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);
function ticks(tag: string, n: number) {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) p = p.then(() => { log.push(tag + i) });
}

async function f() {
  const o = { [await P("b")]: 2 };
  log.push(JSON.stringify(o));
  const p = { x: 1, [(log.push("k"), "y")]: 3, [await P("z")]() { return 4 } };
  log.push(JSON.stringify(p) + (p as any).z());
}

async function* g() {
  const o = { x: 0, [await P("b")]: 2, [(log.push("k2"), "c")]: await P(3) };
  log.push(JSON.stringify(o));
  yield 1;
}

async function main() {
  ticks("f", 4);
  await f();
  log.push("|");
  ticks("g", 4);
  for await (const v of g()) log.push("v" + v);
  console.log(log.join(" "));
}
main();
