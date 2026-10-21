// A top-level `await t.then(f)` where `t` is a plain object with its
// own `then`: the call is the object's, and the await adopts the
// promise it answers.
const log: string[] = [];

function add1(x: number): number {
  return x + 1;
}

const t = {
  then(f: (x: number) => number): Promise<number> {
    log.push("thenable then");
    return Promise.resolve(f(4));
  },
};

Promise.resolve().then(() => log.push("job"));
log.push("r" + (await t.then(add1)));
console.log(log.join(","));
