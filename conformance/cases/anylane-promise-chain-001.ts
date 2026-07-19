// typed chain results crossing into the any lane (RFC
// 20260720-anylane-promise-methods knife 3): the then/catch kernels
// stamp the cb-leg return repr at the entry (attach-before-settle)
// and per settle path at dispatch; finally forwards the source stamp
const t1: any = Promise.resolve(10).then((v: number) => v + 1);
t1.then((v: any) => { console.log("chain-then", v); });
const t2: any = Promise.resolve("s").then((v: string) => v + "!");
t2.then((v: any) => { console.log("chain-str", v); });
const t3: any = Promise.reject("r3").catch((e: any) => "rescued");
t3.then((v: any) => { console.log("chain-catch", v); });
const t4: any = Promise.resolve(4).finally(() => {});
t4.then((v: any) => { console.log("chain-finally", v); });
// any-lane finally: forward + cb-throw-wins + non-callable
const f1: any = Promise.resolve(5);
f1.finally(() => { console.log("fin"); }).then((v: any) => { console.log("fin-fwd", v); });
const f2: any = Promise.reject("bad");
f2.finally(() => {}).catch((e: any) => { console.log("fin-err", e); });
const f3: any = Promise.resolve(6);
f3.finally(() => { throw "fboom"; }).catch((e: any) => { console.log("fin-throw", e); });
console.log("sync");
