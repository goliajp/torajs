// A function whose return type the compiler inferred from the object
// literal it builds returns that object as it is, even after the object
// was given an accessor: only a return type the program wrote is
// checked. The iterator shape is the one Promise.all and its siblings
// read `value` from.

function step(n: number) {
  var v = { done: false };
  Object.defineProperty(v, "value", {
    get() {
      if (n > 1) throw new Error("value " + n);
      return n * 10;
    },
  });
  return v;
}
const a = step(1);
console.log((a as any).value);
try {
  console.log((step(2) as any).value);
} catch (e) {
  console.log("caught", (e as Error).message);
}

const iterable: any = {
  [Symbol.iterator]() {
    return {
      next() {
        var v = { done: false };
        Object.defineProperty(v, "value", {
          get() {
            throw new Error("next value");
          },
        });
        return v;
      },
    };
  },
};
Promise.all(iterable).then(
  () => console.log("resolved"),
  (e: any) => console.log("rejected", e.message),
);

const arrowStep = (n: number) => {
  const v: any = { done: n > 0 };
  Object.defineProperty(v, "value", { get: () => n + 1 });
  return v;
};
console.log(arrowStep(1).done, arrowStep(2).value);
