function topfn(a: number, b: number): number { return a + b; }
function noargs(): number { return 7; }
function withDefault(a: number, b: number = 5): number { return a + b; }
const t: any = topfn;
console.log(t.name);
console.log(t.length);
console.log(typeof t.name);
console.log(typeof t.length);
console.log(t);
const n0: any = noargs;
console.log(n0.name);
console.log(n0.length);
const wd: any = withDefault;
console.log(wd.name);
console.log(wd.length);
const d: any = { name: "custom" };
console.log(d.name);
console.log(t(1, 2));
console.log(topfn(3, 4));
