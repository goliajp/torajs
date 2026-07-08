// TS `Function` — the top callable type (chunk 683). Collapses to
// Any; calls ride the any-call runtime dispatch.
function h(cb: Function) { return cb(1, 2); }
console.log(h((a: number, b: number) => a + b));
function k(cb: Function) { return cb("x"); }
console.log(k((s: string) => s + "!"));
function m(cb: Function) { return cb(); }
console.log(m(() => 7));
const f: Function = (n: number) => n * 3;
console.log(f(14));
