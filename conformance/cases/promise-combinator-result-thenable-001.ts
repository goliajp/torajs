// §27.2.1.3 step 2.f at a combinator's RESULT — the capability's
// resolve function reads `then` off whatever it is handed, and a
// combinator hands it an array. In every ordinary program that array
// is a plain array; `Array.prototype.then` makes it a thenable, and
// the spec adopts it. tr fulfilled with the array instead.
//
// The empty and non-empty inputs reach this through two different
// paths — one settles synchronously, the other a microtask later —
// so both spellings are here.
const log: string[] = [];

// Tick order FIRST, while the prototype is still clean. Both
// positions are load-bearing and were tuned case by case: the empty
// answer lands before a plain `Promise.resolve(0).then`, a non-empty
// one after it.
Promise.all([]).then(() => { log.push("empty"); });
Promise.all([1]).then(() => { log.push("nonempty"); });
Promise.resolve(0).then(() => { log.push("r1"); })
  .then(() => { log.push("r2"); })
  .then(() => {
    console.log("order", log.join(","));
    adopt();
  });

function adopt(): void {
  (Array.prototype as any).then = function (res: any) { res("ADOPTED"); };
  Promise.all([]).then((v: any) => console.log("all-empty", v));
  Promise.allSettled([]).then((v: any) => console.log("allSettled-empty", v));
  Promise.all([1]).then((v: any) => console.log("all-nonempty", v));
  // `any`'s empty answer is a REJECTION (AggregateError), and
  // rejection performs no thenable absorption — §27.2.1.3.1 is the
  // resolve side only. It must not be adopted.
  Promise.any([]).then(
    () => console.log("any-empty", "WRONG: fulfilled"),
    (e: any) => console.log("any-empty", (e as Error).constructor.name),
  );
}
