// A top-level object-literal accessor that reads `arguments` sees the
// arguments of the call that ran it, on every lane that runs it:
// [[Get]] / [[Set]] through the literal's own type, through `any`,
// through Object.entries / values, spread and JSON.stringify, and a
// descriptor's function called with any count.

const o = {
  tag: "t",
  get s() { return this.tag + arguments.length; },
  set s(v: string) { this.tag = v + arguments.length + String(arguments[0]); },
  get g() { return arguments.length; },
};
o.s = "ab";
console.log(o.s, o.tag, o.g);
console.log(JSON.stringify(Object.entries(o)), JSON.stringify(Object.values(o)));
console.log(JSON.stringify({ ...o }), JSON.stringify(o));
(o as any).s = "cd";
console.log((o as any).s, (o as any).g);
const d = Object.getOwnPropertyDescriptor(o, "s");
d.set.call(o, "x", 2, 3);
console.log(o.tag, d.get.call(o, 1, 2));

const a: any = {
  n: 1,
  set s(v: number) { console.log("any set", arguments.length, arguments[0], this.n); },
  get g() { return arguments.length + this.n; },
};
a.s = 5;
console.log("any get", a.g);

let acc = 0;
for (let i = 0; i < 1000; i++) {
  o.s = "k" + (i % 7);
  acc += o.s.length;
}
console.log(acc);
