// A block binding that shadows a namespace-static alias is its own
// function: its .length and call / apply read it, not the outer alias.
// The outer alias is back when the block closes.
const r = Promise.resolve;
const mx = Math.max;
{
  const r = (x: number, y: number) => x * 2 + y;
  console.log(r.call(null, 5, 1), r.length);
  const mx = (a: number) => a;
  console.log(mx.length, mx.apply(null, [4]));
}
console.log(r.length, mx.length);
r.call(Promise, 3).then((v: any) => console.log("outer after block", v));
console.log(mx.call(null, 2, 9));
if (r.length === 1) {
  const r = (a: number, b: number, c: number) => a + b + c;
  console.log("if block", r.length, r.call(null, 1, 2, 3));
}
r.apply(Promise, [4]).then((v: any) => console.log("outer after if", v));
