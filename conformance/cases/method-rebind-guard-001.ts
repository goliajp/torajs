class C {
  v = -1;
  m() { return this.v; }
  getM(): any { return this.m; }
}
const c = new C();
console.log(c.m());
const f: any = c.getM();
console.log(f.call({ v: 42 }));
console.log(f.call(c));
const sub = { v: 7, extra: true };
console.log(f.call(sub));
