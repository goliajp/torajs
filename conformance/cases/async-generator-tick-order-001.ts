// Tick order of an async generator's requests against a plain promise
// chain — the shape every test262 ordering case observes.
// - `return e` awaits e at the return site (§27.6.3.2): the finally
//   around it runs after that await, and its value is the awaited one.
// - a body that runs off its end, and a completed generator's tail
//   step, complete in the same job (no await).
// - `return(v)` awaits v before the body resumes with the return
//   completion (AsyncGeneratorUnwrapYieldResumption), so a finally at
//   the suspension point runs one tick after the call.
// - queued requests are drained in the job that completes the one
//   before them (AsyncGeneratorCompleteStep → resume-next).
const t: string[] = [];
Promise.resolve().then(() => t.push("t1")).then(() => t.push("t2")).then(() => t.push("t3")).then(() => t.push("t4")).then(() => t.push("t5")).then(() => console.log(t.join(" ")));
async function* a() { return 7; }
async function* b() { yield 1; }
async function* c() { try { return Promise.resolve(8); } finally { t.push("cfin"); } }
async function* d() { try { yield 1; } finally { t.push("dfin"); } }
async function* e() { yield 1; yield 2; yield 3; }
a().next().then((r: any) => t.push("a:" + r.value + r.done));
const bi = b(); bi.next(); bi.next().then((r: any) => t.push("b2:" + r.value + r.done)); bi.next().then((r: any) => t.push("b3:" + r.done));
c().next().then((r: any) => t.push("c:" + r.value + r.done));
const di = d(); di.next(); di.return(Promise.resolve(3)).then((r: any) => t.push("dret:" + r.value + r.done));
const ei = e(); ei.next().then((r: any) => t.push("e1:" + r.value)); ei.next().then((r: any) => t.push("e2:" + r.value)); ei.next().then((r: any) => t.push("e3:" + r.value)); ei.next().then((r: any) => t.push("e4:" + r.done));
