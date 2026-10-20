function add1(a: number): number {
  return a + 1;
}
const orig = Promise.prototype.then;
Promise.prototype.then = function (a: any, b: any) {
  console.log("patched");
  return orig.call(this, a, b);
};
const p: Promise<number> = Promise.resolve(6);
p.then(add1).then((v) => console.log(v));
