// exit-code: 13
// `for await` at the top level over an array of typed `Promise<boolean>`
// elements, one still pending once every queued job has run: it cannot
// settle any more, so the module body never goes on (§16.2.1.5.3) and
// the loop body never sees a `false` that does not exist; neither the
// `catch` around the loop nor anything after it runs. The program ends
// with the job queue empty and exits 13 with a warning on stderr, as
// Node does for an unsettled top-level await.
//
// bun spins on the pending element, so the expected output is pinned
// beside this file.
function neverBool(): Promise<boolean> {
  return new Promise(() => {});
}

console.log("before");
try {
  for await (const b of [Promise.resolve(true), neverBool()]) {
    console.log("boolean element", b);
  }
  console.log("boolean loop finished");
} catch (e) {
  console.log("boolean raised", e);
}
console.log("unreachable");
