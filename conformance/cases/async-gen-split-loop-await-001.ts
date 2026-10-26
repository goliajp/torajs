// A `for … of s.split(sep)` in an async generator whose string or
// separator awaits evaluates both once, in order, ahead of the loop, and
// suspends there.

const log: string[] = [];
const P = (v: any) => Promise.resolve(v);
async function* g() {
  for (const p of (await P("a,b")).split(",")) {
    log.push(p);
    yield p;
  }
  for (const q of (log.push("s"), "x-y").split(await P("-"))) {
    log.push(q);
    yield q;
  }
}
async function main() {
  Promise.resolve().then(() => log.push("t0")).then(() => log.push("t1")).then(() => log.push("t2"));
  for await (const v of g()) log.push("v" + v);
  console.log(log.join(" "));
}
main();
