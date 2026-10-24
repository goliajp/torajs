// exit-code: 1
// A throw out of the module body after a top-level await is uncaught:
// the program reports it and exits 1, after the jobs already queued
// have run (§16.2.1.5.3 rejects the module's evaluation; the job queue
// still drains, §9.5). Code after the throw does not run.
Promise.resolve(0).then(() => console.log("job before"));
const v: number = await Promise.resolve(41);
console.log("after await", v + 1);
Promise.resolve(0).then(() => console.log("job after"));
if (v === 41) throw new Error("top-level failure");
console.log("unreachable");
