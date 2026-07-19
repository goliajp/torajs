// Member-store face: a this-using fn-expr stored as an expando method
// on an any receiver (`o.m = function () { ...this... }`) promotes —
// the stored closure carries FLAG_CLOSURE_n and every call rides the
// runtime any-method dispatch's receiver-first channel. Covers dynobj
// and Number/String wrapper receivers, a this-free fn-expr (keeps the
// plain closure ABI), and re-assignment of the same slot.
const nw: any = new Number(6);
nw.tag = "N";
nw.m = function () {
  return this.tag + ":" + this.valueOf();
};
console.log(nw.m());
const sw: any = new String("ab");
sw.suffix = "!";
sw.decorate = function () {
  return this.valueOf() + this.suffix;
};
console.log(sw.decorate());
const o: any = {};
o.name = "d";
o.greet = function () {
  return "hi " + this.name;
};
console.log(o.greet());
o.greet = function () {
  return this.name + "!";
};
console.log(o.greet());
const p: any = {};
p.f = function () {
  return 41 + 1;
};
console.log(p.f());
