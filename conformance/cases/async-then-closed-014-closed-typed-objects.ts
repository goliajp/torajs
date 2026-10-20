// A program that gives no object a `then` (ECMA-262 §27.7.5.3,
// §27.2.1.3.2): an awaited object settles as itself one job later and
// keeps its type, a plain object, a class instance, an array, a
// function, a closure, a Date and a RegExp alike.

type Plain = { a: number; s: string };

class K {
  x = 3;
  name(): string {
    return "k" + this.x;
  }
}

function sq(n: number): number {
  return n * n;
}

const order: string[] = [];

async function main(): Promise<number> {
  const o: Plain = { a: 1, s: "one" };
  const r = await o;
  order.push("o");
  console.log(r.a, r.s, r === o);
  const k = await new K();
  console.log(k.x, k.name());
  const arr = await [1, 2, 3];
  console.log(arr.length, arr[2]);
  const f = await sq;
  console.log(f(5));
  const add = (a: number) => a + k.x;
  const g = await add;
  console.log(g(1));
  const d = await new Date(0);
  console.log(d.getTime());
  const re = await /b+/;
  console.log(re.test("abbc"));
  return r.a + k.x;
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

main().then((v) => console.log("main", v, order.join(",")));
tick();
