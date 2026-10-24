// exit-code: 1
// A top-level await of a promise a job rejects later, outside any
// `try`, throws its reason out of the module body: uncaught, exit 1.
// The jobs queued before the await still run first.
Promise.resolve(0).then(() => console.log("job 1"));
const ok: string = await Promise.resolve("first");
console.log("ok", ok);
const p: Promise<number> = new Promise((_, rej) => queueMicrotask(() => rej(new Error("late"))));
Promise.resolve(0).then(() => console.log("job 2"));
const n: number = await p;
console.log("unreachable", n);
