// a builtin whose argument is a temporary the caller owns (an object
// literal, a fresh string) and whose kernel raises from user code it
// ran (valueOf / toString / a getter / toJSON / a proxy trap): the
// raise is caught, the answer is the error, and the temporary is
// released on that edge too (the leak probes for these sites measure
// a flat RSS over a million raises)
function probe(f: () => unknown): string {
  try {
    f();
    return "ok";
  } catch (e) {
    return (e as Error).message;
  }
}
console.log(probe(() => Number({ valueOf() { throw new Error("valueOf"); } })));
console.log(probe(() => String({ toString() { throw new Error("toString"); } })));
console.log(probe(() => Object.values({ get a() { throw new Error("values getter"); } })));
console.log(probe(() => Object.entries({ get a() { throw new Error("entries getter"); } })));
console.log(probe(() => Object.keys(new Proxy({ a: 1 }, { ownKeys() { throw new Error("ownKeys"); } }))));
console.log(probe(() => JSON.stringify({ toJSON() { throw new Error("toJSON"); } })));
console.log(probe(() => Object.assign({}, { get a() { throw new Error("assign getter"); } })));
console.log(probe(() => Object.freeze(new Proxy({ a: 1 }, { preventExtensions() { throw new Error("preventExtensions"); } }))));
console.log(probe(() => Symbol.for({ toString() { throw new Error("symbol key"); } } as any)));
// the same shapes without a raise keep answering
console.log(Number({ valueOf() { return 7; } }), String({ toString() { return "s"; } }), Object.keys({ a: 1, b: 2 }).join(","), JSON.stringify({ toJSON() { return 3; } }));
