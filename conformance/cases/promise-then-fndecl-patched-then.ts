// a program that patches Promise.prototype.then keeps a top-level
// handler wrapped: the call goes down the any-method lane
function add1(v: number): number {
  return v + 1;
}
const orig = Promise.prototype.then;
let seen = 0;
Promise.prototype.then = function (this: any, a: any, b: any): any {
  seen++;
  return orig.call(this, a, b);
};
async function main(): Promise<void> {
  const v = await Promise.resolve(1).then(add1);
  console.log(v, seen > 0);
}
main();
