// An await inside a generic function's closure, in a program whose
// only form the proof needs types for is an `as` of an `any` to an
// object type: the base run types the await of `T` as `T`, which still
// asks the proof under the run's types, where the cast opens it. The
// value under the object type is a promise, which the await adopts
// (ECMA-262 §27.7.5.3, §27.2.1.3.2 steps 9-13), one job after a plain
// object would settle.

type Plain = { a: number };

const order: string[] = [];

function wrap<T>(x: T): Promise<T> {
  const run = async () => {
    const v = await x;
    order.push("v");
    return v;
  };
  return run();
}

const hidden: any = Promise.resolve(7);
const p = hidden as Plain;
const o: Plain = { a: 1 };

wrap(p).then((v) => console.log("p", typeof v, String(v)));
wrap(o).then((v) => console.log("o", typeof v, v.a, v === o));
Promise.resolve().then(() => order.push("t1")).then(() => order.push("t2"));
Promise.resolve().then(() => 0).then(() => 0).then(() => 0).then(() => console.log(order.join(",")));
