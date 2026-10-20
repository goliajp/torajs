// An await of an element read past an array's end, in a program that
// gives no object a `then`: the element's type is an object type, its
// value `undefined` (ECMA-262 §27.2.1.3.2 step 8: a resolution that is
// not an Object fulfills, one job later).

type Plain = { a: number };

class K {
  k = 1;
}

const order: string[] = [];

async function main() {
  const arr: Plain[] = [{ a: 1 }];
  const v = await arr[3];
  order.push("v");
  console.log(v === undefined, typeof v);
  const ks: K[] = [];
  const w = await ks[0];
  console.log(w === undefined, typeof w);
  const x = await arr[0];
  console.log(x.a, x === arr[0]);
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

main().then(() => console.log(order.join(",")));
tick();
