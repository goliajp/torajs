// exit-code: 13
// `await` of an any-typed promise that is still pending once every
// queued job has run: nothing left can settle it, so the module body
// never goes on (§16.2.1.5.3: its evaluation stays pending). The
// `catch` around the await does not run: no completion ever reaches
// it. The program ends with the job queue empty and exits 13 with a
// warning on stderr, as Node does for an unsettled top-level await. A
// promise a queued job settles still answers its value.
//
// bun spins on the second await, so the expected output is pinned
// beside this file.
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
