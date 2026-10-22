// A handler's `any` return that is a thenable is adopted through its
// own `then` (§27.2.1.3.2 steps 8-9), one job later than a plain value.
function toThenable(v: number): any {
  return {
    then(res: (x: number) => void) {
      console.log("then called");
      res(v + 1);
    },
  };
}

Promise.resolve(1)
  .then(toThenable)
  .then((x: number) => console.log("adopted", x));
Promise.resolve(0)
  .then(() => console.log("tick 1"))
  .then(() => console.log("tick 2"))
  .then(() => console.log("tick 3"));
