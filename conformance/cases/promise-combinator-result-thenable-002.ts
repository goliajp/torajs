// The abrupt half of `promise-combinator-result-thenable-001`: when
// the `then` READ on the result array throws, §27.2.1.3 step 2.g
// rejects the combinator's promise with what it threw. tr fulfilled
// anyway — a failure turned into a success, which no fixture had the
// shape to catch.
//
// Its own file because the getter has to be installed before any
// combinator in the program settles; sharing a file with the
// adoption cases let one of theirs land on this prototype.
Object.defineProperty(Array.prototype, "then", {
  configurable: true,
  get() { throw new Error("poison"); },
});

Promise.all([]).then(
  () => console.log("all-empty", "WRONG: fulfilled"),
  (e: any) => console.log("all-empty", (e as Error).message),
);
Promise.allSettled([]).then(
  () => console.log("allSettled-empty", "WRONG: fulfilled"),
  (e: any) => console.log("allSettled-empty", (e as Error).message),
);
Promise.all([1]).then(
  () => console.log("all-nonempty", "WRONG: fulfilled"),
  (e: any) => console.log("all-nonempty", (e as Error).message),
);
