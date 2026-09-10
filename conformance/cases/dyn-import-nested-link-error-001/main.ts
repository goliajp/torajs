// §16.2.1.6.4 step 7 runs for every module in a candidate's graph:
// b.ts asks c.ts for a name it never exports, so import("./b.ts")
// rejects with the link-phase SyntaxError. bun's transpiler drops
// the unused import and resolves — node / spec reject.
import("./b.ts").then(
  () => console.log("resolved"),
  (e: any) => console.log("caught", e instanceof SyntaxError),
);
