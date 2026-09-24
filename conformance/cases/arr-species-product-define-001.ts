// §23.1.3.x CreateDataPropertyOrThrow into the species product: an
// array the species constructor pre-defined non-writable (but
// configurable) is REDEFINED, not assigned; a non-configurable index
// refuses with a TypeError.
function mk(configurable: boolean) {
  const a: any = [1];
  a.constructor = {};
  a.constructor[Symbol.species] = function (len: any) {
    const q: any = new Array(0);
    Object.defineProperty(q, 0, { value: 0, writable: false, configurable, enumerable: false });
    return q;
  };
  return a;
}
for (const configurable of [true, false]) {
  for (const [name, run] of [
    ["map", (a: any) => a.map(() => 2)],
    ["filter", (a: any) => a.filter(() => true)],
    ["slice", (a: any) => a.slice(0)],
    ["splice", (a: any) => a.splice(0, 1)],
  ] as any) {
    try {
      const r = run(mk(configurable));
      console.log(configurable, name, JSON.stringify(Object.getOwnPropertyDescriptor(r, 0)));
    } catch (e: any) {
      console.log(configurable, name, e.constructor === TypeError ? "TypeError" : "other");
    }
  }
}
