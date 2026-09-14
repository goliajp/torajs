// A `this`-reading function expression written straight into an
// argument of a call whose CALLEE is an `any` binding (a `: any`
// variable, or a parameter annotated `any` everywhere it is declared)
// is a receiver-safe position: every call of an `any` value rides the
// any lane, and every path that lane takes with the argument reads the
// receiver-first flag before seeding argv[0]. Before rotation 649 the
// program was refused as an unclaimed receiver position (the executor
// shape of test262 Promise/reject/capability-invocation — which then
// moves on to `Promise.reject.call(P, …)` with a function-valued
// `this`, a separate, registered gap).
function run(executor: any): void { executor(function () {}, function (this: any) { console.log("plain", this === undefined); }); }
run(function (a: any, b: any) { b(); });
run(function (a: any, b: any) { b.call({ k: 1 }); });
const viaVar: any = function (cb: any, o: any) { return cb.call(o, 2); };
console.log(viaVar(function (this: any, v: number): number { return v + (this as any).k; }, { k: 5 }));
function keep(cb: any): any { const held: any = cb; return held.apply({ k: 7 }, [1]); }
console.log(keep(function (this: any, v: number): number { return v * (this as any).k; }));
