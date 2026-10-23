// §9.13 — a target held only by a top-level binding of a module that
// awaits stays alive to the program's exit, and the exit runs no cleanup
// job for it: the spec does not promise one, and bun and node run none.
const fr: FinalizationRegistry<string> = new FinalizationRegistry((held: string) => {
  console.log("cleanup", held);
});
const target: { n: number } = { n: 1 };
fr.register(target, "top-level target");
await Promise.resolve();
console.log("after await", target.n);
