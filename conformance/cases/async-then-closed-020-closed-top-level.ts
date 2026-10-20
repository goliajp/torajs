// Awaits of objects at a module's top level (ECMA-262 §16.2.1.5.3) in
// a program that gives no object a `then`: each settles as itself and
// keeps its type, after the jobs queued before it have run.

type Plain = { a: number };

class K {
  x = 3;
}

function sq(n: number): number {
  return n * n;
}

const order: string[] = [];
Promise.resolve(0).then(() => order.push("job"));
const o: Plain = { a: 1 };
const r = await o;
order.push("o");
console.log(r.a, r === o);
const k = await new K();
console.log(k.x);
const xs = await [4, 5];
console.log(xs[1]);
const f = await sq;
console.log(f(3));
console.log(order.join(","));
export {};
