// A top-level `await p.then(f)` where a still-pending `p` carries its
// own `then`, defined as a property: the call is the property's
// (§27.2.5.1 Invoke), and the await reads what it answers.
const log: string[] = [];

function f(v: number): number {
  log.push("f" + v);
  return v + 1;
}

let settle: (v: number) => void = () => {};
const p = new Promise<number>((r) => {
  settle = r;
});
Object.defineProperty(p, "then", {
  value: function (cb: any) {
    log.push("own then");
    return Promise.resolve(9).then(cb);
  },
});
queueMicrotask(() => settle(1));
Promise.resolve().then(() => log.push("job"));
const r = await p.then(f);
log.push("r" + r);
console.log(log.join(","));
