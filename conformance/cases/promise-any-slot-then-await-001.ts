// `await <anyPromise>.then(cb)` — the `.then` on an `any` receiver is
// served by the runtime any-lane bridge, whose result cell stores a
// BOXED settled value; awaiting it must go by value, not through the
// typed promise read.
async function main() {
  var p = Promise.resolve(9);

  console.log("inline", await p.then((v: any) => v + 1));

  const bound = await p.then((v: any) => v + 1);
  console.log("bound", bound);

  console.log("nested", await p.then((v: any) => v + 1).then((v: any) => v * 2));

  console.log("string cb", await p.then((v: any) => "s" + v));

  // an empty handler slot forwards the source settlement untouched
  console.log("catch forward", await p.catch((e: any) => 0));

  // the typed receiver keeps the typed read
  const q = Promise.resolve(9);
  console.log("typed recv", await q.then((v: number) => v + 1));
}
main();
