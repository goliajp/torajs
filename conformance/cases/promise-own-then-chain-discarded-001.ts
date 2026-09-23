// §27.2.5.1 — `p.then(f)` over a cell carrying an own `then` is an
// Invoke of that override, and the override answers whatever it
// returns. tr's typed chain lane has to put that answer in a
// `Type::Promise` slot, so a non-promise is a loud TypeError. In
// STATEMENT position there is no slot — the statement reads nothing
// back — and the answer is simply released.

const p1: any = Promise.resolve(1);
p1.then = function (res: any) { res("OWN1"); };
Promise.resolve(p1).then((v: any) => console.log("stmt", v));

// The same through a typed binding whose `then` was patched later.
const q: Promise<number> = Promise.resolve(1);
(q as any).then = function (res: any) { res("OWN2"); };
q.then((v: any) => console.log("typed-var", v));

// `.finally` Invokes `then` per spec, same road.
const p3: any = Promise.resolve(1);
p3.then = function (res: any) { res("OWN3"); };
Promise.resolve(p3).finally(() => console.log("finally"));

// An override answering a non-promise of any shape is fine here.
const p4: any = Promise.resolve(1);
p4.then = function (res: any) { res("OWN4"); return 7; };
Promise.resolve(p4).then((v: any) => console.log("returns-number", v));

// An override that DOES answer a promise keeps working in either
// position, so the chain below still reads a real one.
const p5: any = Promise.resolve(1);
p5.then = function (res: any) { res("OWN5"); return Promise.resolve(9); };
const kept = Promise.resolve(p5).then((v: any) => console.log("returns-promise", v));
kept.then((n: any) => console.log("kept", n));
