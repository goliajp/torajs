// Dynamic import nests as a CallExpression (§13.3.10 — ImportCall is
// a CallExpression). The inner import answers a Promise; the outer
// call stringifies it ("[object Promise]"), matches no module, and
// rejects — while the inner namespace still loads.
import(import("./mod.ts")).catch(() => {
  console.log("outer rejected");
});
import("./mod.ts").then((ns: any) => {
  console.log("inner", ns.inner);
});
