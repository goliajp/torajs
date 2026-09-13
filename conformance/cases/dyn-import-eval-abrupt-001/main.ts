// A dynamic import EVALUATES its target's module body once, when the
// import runs (§16.2.1.5.3): an abrupt completion rejects the promise
// with the thrown value — and a second import of the same module
// rejects with the same error without running the body again — while
// a body that completes hands back the namespace. The interleaving
// of INDEPENDENT loads is host-defined (bun's bundler settles them
// across several turns); `main.expected` pins tr's, where each
// import's body runs in the job its `import()` queued.
console.log("before");
import("./throws.js").then(
  () => console.log("throws resolved"),
  (e: any) => console.log("throws rejected", e instanceof TypeError, e.message),
);
import("./throws.js").then(
  () => console.log("throws resolved again"),
  (e: any) => console.log("throws rejected again", e instanceof TypeError),
);
import("./sidefx.js").then(
  (ns: any) => console.log("sidefx", ns.count),
  (e: any) => console.log("sidefx rejected", String(e)),
);
import("./sidefx.js").then((ns: any) => console.log("sidefx again", ns.count));
console.log("after");
