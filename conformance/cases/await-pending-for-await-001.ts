// `for await` over an array of typed promises: an element still
// pending once every queued job has run raises instead of binding
// `0` / `false`, so the loop body never runs on a value that does not
// exist. Settled elements before it still bind their values.
//
// The raise is a transition: the specification suspends here forever
// (the module never finishes evaluating), which needs the event loop
// this runtime does not have yet. bun spins on the pending element, so
// the expected output is pinned beside this file.
function neverNum(): Promise<number> {
  return new Promise(() => {});
}
function neverBool(): Promise<boolean> {
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
  console.log("number raised", e instanceof TypeError);
}
try {
  for await (const b of [neverBool()]) {
    console.log("boolean element", b);
  }
  console.log("boolean loop finished");
} catch (e) {
  console.log("boolean raised", e instanceof TypeError);
}
