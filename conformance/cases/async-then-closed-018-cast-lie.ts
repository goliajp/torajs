// A promise under an object type (`as` of an `any`) is adopted
// (ECMA-262 §27.2.1.3.2 steps 9-13), and its value, an object of
// another layout, is what the await answers.

type Plain = { a: number; s: string };

const lie: any = Promise.resolve({ a: 7, s: "lied" });
const cast = lie as Plain;
const order: string[] = [];

async function main() {
  const o: Plain = { a: 1, s: "one" };
  const r = await o;
  order.push("o");
  const c = await cast;
  order.push("c");
  console.log(r.a, c.a, c.s);
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
  await null;
  order.push("t3");
}

main().then(() => console.log(order.join(",")));
tick();
