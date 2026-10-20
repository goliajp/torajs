// An await inside a generic function's closure, in a program that
// gives no object a `then`: the specialization that binds the type
// parameter to an object type awaits it as itself, one job later
// (ECMA-262 §27.7.5.3, §27.2.1.3.2 step 8 onwards).

type Plain = { a: number; s: string };

const order: string[] = [];

function wrap<T>(x: T): Promise<T> {
  const run = async () => {
    const v = await x;
    order.push("v");
    return v;
  };
  return run();
}

async function tick() {
  order.push("t1");
  await null;
  order.push("t2");
}

const o: Plain = { a: 1, s: "one" };
wrap(o).then((v) => console.log(typeof v, v.a, v.s, v === o, order.join(",")));
wrap([1, 2, 3]).then((xs) => console.log(xs.length, xs[2]));
tick();
