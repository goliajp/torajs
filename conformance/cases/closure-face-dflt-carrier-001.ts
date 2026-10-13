// A closure's fn-typed parameter with a default holds what its callers
// hand it: an `any` value, or a function declared to return `any`, is
// called by its own convention and answers what it returns, while the
// default and a function of the annotation's face keep the typed call.
type F = (x: number) => number;
const p = { g: (x: number) => x * 3 };
const g = p.g;
function mkS(): (x: number) => any { return (x: number) => "s" + x }
const raw: any = mkS();
const w = (h: F = g) => h(7);
const v = w(raw);
console.log(w(), typeof v + ":" + v);
const z = (h: F) => h(7);
const v2 = z(raw);
console.log(typeof v2 + ":" + v2);
const v3 = w(mkS());
console.log(typeof v3 + ":" + v3);
const t = (h: F = g) => h(2) + 1;
console.log(t(), t(g), t(undefined), t((x: number) => x));
