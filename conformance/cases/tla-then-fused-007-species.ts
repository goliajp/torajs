// A top-level `await p.then(f)` after the program replaced
// `Promise[Symbol.species]`: `then` reads it to build the derived
// promise (§27.2.5.4 step 3, SpeciesConstructor), so the getter runs
// once per call.
const log: string[] = [];

function add1(x: number): number {
  return x + 1;
}

Object.defineProperty(Promise, Symbol.species, {
  get() {
    log.push("species");
    return Promise;
  },
});

log.push("a" + (await Promise.resolve(1).then(add1)));
const p = Promise.resolve(5);
log.push("b" + (await p.then(add1)));
console.log(log.join(","));
