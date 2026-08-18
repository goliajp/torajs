// proposal-array-from-async §2.1.1 step 3.j.ii.6.a — the mapped
// form is Call(mapfn, thisArg, «value, k»): a third argument to
// Array.fromAsync is the mapfn's receiver, not eval-and-drop. The
// mapped dyn kernel used to route every call through the
// this-undefined lane, so a fn-expr mapfn reading `this` saw the
// strict undefined even when the call site passed a receiver.
async function main() {
  // thisArg bound: the fn-expr mapfn reads a field off it.
  const scaled = await Array.fromAsync(
    [1, 2, 3],
    function (v: any, i: any) {
      return v * this.factor + i;
    },
    { factor: 10 },
  );
  console.log(scaled.join(","));

  // settled-promise elements interleave with the bound receiver.
  const mixed = await Array.fromAsync(
    [Promise.resolve(5), 6],
    function (v: any) {
      return this.prefix + v;
    },
    { prefix: "p" },
  );
  console.log(mixed.join(","));

  // no thisArg: §10.2.1.2 strict — this stays undefined.
  const kinds = await Array.fromAsync([0], function () {
    return typeof this;
  });
  console.log(kinds[0]);

  // arrow mapfn ignores thisArg by construction (lexical this).
  const arrows = await Array.fromAsync([7], (v: any) => v + 1, { factor: 99 });
  console.log(arrows[0]);
}
main();
