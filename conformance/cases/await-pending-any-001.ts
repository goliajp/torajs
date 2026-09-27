// `await` of an any-typed promise that is still pending once every
// queued job has run: nothing left can settle it, and this await
// cannot suspend yet, so it raises instead of answering undefined —
// the code after the await never runs on a value that does not exist.
// A promise a queued job settles still answers its value.
//
// The raise is a transition: the specification suspends here forever
// (the module never finishes evaluating), which needs the event loop
// this runtime does not have yet. bun spins on the second await, so
// the expected output is pinned beside this file.
let res: any;
const p: any = new Promise((r) => { res = r; });
queueMicrotask(() => res(7));
const v = await p;
console.log("settled", v);
const never: any = new Promise(() => {});
try {
  const x = await never;
  console.log("after never", x);
} catch (e) {
  console.log("pending raised", e instanceof TypeError);
}
