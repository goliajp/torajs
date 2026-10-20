// An await inside a generic function's closure, of an object the
// program gives a `then` under a key built at run time (ECMA-262
// §27.7.5.3, §27.2.1.3.2 steps 9-13): every specialization of the
// body sees the same program.

function wrap<T>(x: T): Promise<T> {
  const run = async () => {
    const v = await x;
    return v;
  };
  return run();
}

const o = { a: 1 };
const k: string = ["n", "e", "h", "t"].reverse().join("");
const oa: any = o;
oa[k] = (r: (v: number) => void) => r(42);
wrap(o).then((v) => console.log(typeof v, v));
