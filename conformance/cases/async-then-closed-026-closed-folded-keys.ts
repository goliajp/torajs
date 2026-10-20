// Computed keys spelled with literals the walk folds into one key
// (ECMA-262 §13.15.2: `"a" + "b"`, `-1`, `1 + 1`), in a program that
// gives no object a `then`: an awaited object settles as itself one job
// later and keeps its type.

type Plain = { a: number; ab: number };

const order: string[] = [];

async function main() {
  const o: Plain = { a: 1, ab: 2 };
  const oa: any = o;
  oa["a" + "b"] = 5;
  oa[-1] = 6;
  const xs = [10, 20, 30];
  xs[1 + 1] = 33;
  const r = await o;
  order.push("o");
  console.log(r.a, r.ab, oa[-1], xs[2], r === o);
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

main().then(() => console.log(order.join(",")));
tick();
