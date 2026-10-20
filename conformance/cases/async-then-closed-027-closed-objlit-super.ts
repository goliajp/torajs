// An object literal's method reads `super` (ECMA-262 §13.3.7: the base
// is GetPrototypeOf of the literal, %Object.prototype% here), in a
// program that gives no object a `then`: an awaited object settles as
// itself one job later and keeps its type (§27.2.1.3.2 step 8 onwards).

type Plain = { a: number };

const order: string[] = [];

const o = {
  a: 1,
  m() {
    return typeof super.toString;
  },
};

async function main() {
  const p: Plain = { a: 2 };
  const r = await p;
  order.push("p");
  console.log(r.a, r === p, o.m());
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

main().then(() => console.log(order.join(",")));
tick();
