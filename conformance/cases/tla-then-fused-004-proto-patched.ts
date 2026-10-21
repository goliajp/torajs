// `Promise.prototype.then` replaced by the program: a top-level
// `await p.then(f)` calls the replacement (§13.3.6.1 reads `then` off
// the receiver's prototype), for a fresh fulfilled receiver and for a
// pending one alike.
const orig = Promise.prototype.then;
Promise.prototype.then = function (a: any, b: any) {
  console.log("patched");
  return orig.call(this, a, b);
};

function add1(x: number): number {
  return x + 1;
}

console.log(await Promise.resolve(1).then(add1));

let settle: (v: number) => void = () => {};
const p = new Promise<number>((r) => {
  settle = r;
});
queueMicrotask(() => settle(10));
console.log(await p.then(add1));
