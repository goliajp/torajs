// a handler that only throws makes the derived promise reject; the
// promise it names still chains, whatever the handler's parameter
// was declared as
Promise.resolve(1).then((v: any) => { throw new Error("x"); }).catch((e) => console.log("f", e.message));
const r = Promise.resolve(1).then((v: any) => { throw new Error("y"); });
r.then(() => console.log("no"), (e) => console.log("h", e.message));
r.catch(() => {}).finally(() => console.log("h-fin"));
Promise.reject(new Error("r")).catch((e) => { throw new TypeError("t"); }).catch((e) => console.log("b", e.name));
Promise.resolve(1).catch((e) => { throw new TypeError("t"); }).catch((e) => console.log("c", e.name)).then((x) => console.log("c2", x));
const p: Promise<number> = Promise.reject(new Error("r"));
p.catch((e) => { throw new TypeError("t"); }).catch((e) => console.log("d", e.name));
Promise.resolve(2).then((v: number) => { throw new Error("g"); }).catch((e) => console.log("g", e.message));
