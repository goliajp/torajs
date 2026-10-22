// A handler returning a plain object: the probe finds no `then`, so the
// derived promise fulfills with the object itself.
type Pair = {
  a: number;
  b: string;
};

function mk(v: number): Pair {
  return { a: v, b: "x" + v };
}

Promise.resolve(5)
  .then(mk)
  .then((p: Pair) => console.log("plain", p.a, p.b));
