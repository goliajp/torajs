// An accessor reading `arguments` on a literal that also spreads
// another object: the layout names the spread's fields, the accessor
// still gets its own arguments.
const base = { a: 1 };
const o = { ...base, get g() { return arguments.length; }, set s(v: number) { console.log("set", arguments.length, arguments[0]); } };
console.log(o.g, o.a);
o.s = 4;
function f() {
  const p = { ...base, get g() { return typeof arguments + arguments.length; } };
  return p.g;
}
console.log(f());
const q = { ...o, get h() { return arguments.length; } };
console.log(q.h);
