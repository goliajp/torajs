// a program that writes Promise.resolve keeps a top-level handler of
// a chain rooted at it wrapped
function add1(v: number): number {
  return v + 1;
}
let calls = 0;
Promise.resolve = function (x: any): any {
  calls++;
  return new Promise((res) => res(x));
} as any;
async function main(): Promise<void> {
  const v = await Promise.resolve(1).then(add1);
  console.log(v, calls);
}
main();
