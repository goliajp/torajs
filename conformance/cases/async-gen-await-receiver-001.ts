// A method call in an async generator whose receiver or arguments
// await keeps its receiver: `(await p).m(x)`, `o[k](await x)`,
// `(await p)[k](await x)`; the receiver and the key are evaluated before
// the arguments.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);
function ticks(n: number) {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) p = p.then(() => { log.push("t" + i) });
}
class Box {
  k: number;
  constructor(k: number) { this.k = k }
  add(x: number) { return this.k + x }
}

async function* g(key: string) {
  const o: any = new Box(1);
  yield (await P(new Box(2))).add(3);
  yield o[key](await P(4));
  yield (await P(new Box(5)))[(log.push("key"), key)](await P(6));
  yield (await P([1, 2])).map((x: number) => x * 10).join("+");
}

async function main() {
  ticks(8);
  for await (const v of g("add")) log.push("v" + v);
  console.log(log.join(" "));
}
main();
