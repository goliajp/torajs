// typed chain calls settle the call site's reference on each handler:
// inline literals, borrowed handlers, and f64 handlers behind the
// bits-ABI wrap, one- and two-argument forms, and the await driver.
const half = (v: number): number => v + 0.5;
let log: string[] = [];
Promise.resolve(1.5).then(half).then((v: number) => log.push("borrowed " + typeof v));
Promise.resolve(2.5).then((v: number): number => v * 2).then((v: number) => log.push("inline " + v));
function twoArg(): void {
  const rej = new Promise<number>((_ok, no) => no(new Error("x")));
  rej.then((v: number) => v, (e: any) => { log.push("two-arg " + e.message); return 0; });
}
twoArg();
Promise.resolve(3).catch((_e: any) => 0).finally(() => log.push("finally"));
async function step(i: number): Promise<number> { await null; return i * 1.5; }
async function main() {
  let s = 0;
  for (let i = 0; i < 1000; i++) s += await step(i);
  log.push("await " + s);
  console.log(log.join("\n"));
  console.log(half(1));
}
main();
