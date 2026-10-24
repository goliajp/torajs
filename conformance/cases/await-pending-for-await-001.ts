// exit-code: 13
// `for await` over an array of typed promises at the top level: an
// element still pending once every queued job has run cannot settle
// any more, so the module body never goes on (§16.2.1.5.3): the loop
// body never runs on a value that does not exist, and neither the
// `catch` around the loop nor anything after it runs. Settled elements
// before it still bind their values. The program ends with the job
// queue empty and exits 13 with a warning on stderr, as Node does for
// an unsettled top-level await.
//
// bun spins on the pending element, so the expected output is pinned
// beside this file.
function neverNum(): Promise<number> {
  return new Promise(() => {});
}

for await (const x of [Promise.resolve(1), Promise.resolve(2)]) {
  console.log("settled", x);
}

try {
  for await (const x of [Promise.resolve(5), neverNum()]) {
    console.log("number element", x);
  }
  console.log("number loop finished");
} catch (e) {
  console.log("number raised", e);
}
console.log("unreachable");
