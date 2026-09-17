const log: string[] = [];
const rej = Promise.reject(new Error("x"));
(rej as Promise<number>).then((v: number) => v, (e: any) => { log.push("rej " + e.message); return 1; });
(Promise.resolve(2) as Promise<number>).then((v: number) => log.push("res " + v));
function f() { const p = Promise.resolve(3); (p as Promise<number>).then((v: number) => log.push("local " + v)); }
f();
setTimeoutLike();
function setTimeoutLike() { Promise.resolve().then(() => Promise.resolve()).then(() => console.log(log.join(","))); }
