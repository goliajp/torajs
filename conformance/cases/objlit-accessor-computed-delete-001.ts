// A delete through a computed key (§13.5.1.2 on a key the program
// computes) removes an accessor member like a named delete does.
function del(x: any, k: string) {
  delete x[k];
}
const o = {
  get n() {
    return 7;
  },
  get s() {
    return "s";
  },
  get b() {
    return { v: 1 };
  },
};
console.log(o.n, o.s, o.b.v);
del(o, "n");
del(o, "b");
console.log(o.n, o.s, o.b, "n" in o);
const k: string = process.argv.length > 99 ? "x" : "n";
const t = {
  get n() {
    return 5;
  },
  get s() {
    return "t";
  },
};
console.log(t.n, t.s);
delete (t as any)[k];
console.log(t.n, t.s, "n" in t);
