// `await p.then(f)` where the cell carries its own `then`: the call is
// the override's (§27.2.5.1 Invoke), and the await reads what it
// answers.
const log: string[] = [];

function f(v: number): number {
  log.push("f" + v);
  return v + 1;
}

const p = Promise.resolve(3);
(p as any).then = function (cb: any) {
  log.push("own then");
  return Promise.resolve(100).then(cb);
};
Promise.resolve().then(() => log.push("job"));
const r = await p.then(f);
log.push("r" + r);
const q = Promise.resolve(7);
const s = await q.then(f);
log.push("s" + s);
console.log(log.join(","));
