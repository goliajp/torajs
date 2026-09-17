// §27.2.4.6 — a rejected promise never fulfils, so its T is
// unconstrained by the rejection reason (TS: Promise.reject<T = never>).
const p: Promise<number> = Promise.reject(new Error("boom"));
p.catch((e: any) => { console.log("caught", e.message) });

const q: Promise<string> = Promise.reject(42);
q.catch((e: any) => { console.log("caught", e) });

async function f(): Promise<number> { throw new Error("inner") }
f().catch((e: any) => { console.log("f", e.message) });

async function g(): Promise<number> { return 7 }
g().then((v: number) => { console.log("g", v + 1) });

const r: Promise<number[]> = Promise.reject();
r.catch((e: any) => { console.log("empty", e) });
