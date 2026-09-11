// §27.2.4.2 / §27.2.3.1 — a Promise's [[Prototype]] is
// %Promise.prototype% whichever path minted it. The any-lane
// getPrototypeOf tag ladder had no Promise arm, so every promise
// answered null while `instanceof Promise` said true (test262's
// Array/fromAsync/returns-promise asserts exactly this pair).
async function af(): Promise<number> {
  return 1;
}
async function main(): Promise<void> {
  const cases: any = {
    resolve: Promise.resolve(1),
    ctor: new Promise((r) => r(1)),
    all: Promise.all([]),
    race: Promise.race([Promise.resolve(1)]),
    asyncfn: af(),
    then: Promise.resolve(1).then((x: any) => x),
    fromAsync: Array.fromAsync([0]),
  };
  for (const k of Object.keys(cases)) {
    const p: any = cases[k];
    console.log(k, Object.getPrototypeOf(p) === Promise.prototype, p instanceof Promise);
  }
  // The wrapper family's missing member, same ladder.
  const s: any = Object(Symbol("s"));
  console.log("symwrap", Object.getPrototypeOf(s) === Symbol.prototype);
  console.log("done");
}
main();
