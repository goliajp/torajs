class C {
  x = 1;
  m() { return 2; }
}
const c = new C();
console.log(c.missing);
console.log(typeof c.missing);
if (c.missing === undefined) { console.log("miss-ok"); }
console.log(c.x);
console.log(c.m());
