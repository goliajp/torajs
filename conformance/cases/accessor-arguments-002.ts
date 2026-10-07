// An object-literal accessor body that reads `arguments` sees the
// arguments of the call that ran it: none for [[Get]], one for [[Set]],
// and whatever was passed when the function is taken out of its
// descriptor and called — top-level literals and a function's own alike.

const o = {
  set s(v: number) { console.log("set", arguments.length, arguments[0]); },
  get g() { return arguments.length; },
};
o.s = 5;
console.log("get", o.g);
const od = Object.getOwnPropertyDescriptor(o, "s");
od.set(1, 2, 3);
console.log("get out", Object.getOwnPropertyDescriptor(o, "g").get(9, 9));

function f() {
  const p = {
    set s(v: number) { console.log("fn set", arguments.length, arguments[0]); },
    get g() { return arguments.length; },
  };
  p.s = 6;
  console.log("fn get", p.g);
  const pd = Object.getOwnPropertyDescriptor(p, "s");
  pd.set(7, 8);
}
f();

const w = {
  n: 1,
  get val() { return this.n * 10 + arguments.length; },
  set val(x: number) { this.n = x + arguments.length; },
};
w.val = 4;
console.log("w.val", w.val);
w.val += 2;
console.log("w compound", w.n, w.val);
w.val = 4.5;
console.log("w float", w.n);
console.log("spread", JSON.stringify({ ...w }));
console.log("json", JSON.stringify(w));
const wd = Object.getOwnPropertyDescriptor(w, "val");
console.log("w get out", wd.get.call(w, 1, 2, 3));
wd.set.call(w, 1, 1);
console.log("w set out", w.n);

const plain = { get k() { return 3; }, set k(v: number) { console.log("plain", v); } };
plain.k = plain.k + 1;
