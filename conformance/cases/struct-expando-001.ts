// RFC 20260714-struct-dynamic-props blade 2 — expando write/read
// through the any lane on typed-struct receivers (four source
// lanes), keyed/numeric/symbol spellings, and the has face.

// named struct variable
const s = { w: 2 };
const o1: any = s;
o1.b = 7;
console.log(o1.b, o1.w);

// class instance
class C {
  v: number = 2;
}
const o2: any = new C();
o2.a = 1;
console.log(o2.a, o2.v);

// rewrite of an existing typed field stays on the layout slot
o2.v = 999;
console.log(o2.v);

// inferred return
function mk() {
  return { p: 1 };
}
const o3: any = mk();
o3.q = 5;
console.log(o3.q, o3.p);

// keyed dynamic-string write (the ctor-injection shape computed
// fields lower to)
const k = "dyn" + "Key";
o2[k] = 42;
console.log(o2[k], o2.dynKey);

// overwrite an expando in place
o2.a = 11;
console.log(o2.a);

// expando holding a heap value (rc traffic through the dict)
o1.s = "he" + "ap";
console.log(o1.s);

// numeric key — §7.1.19 canonical decimal spelling
o3[4] = 40;
console.log(o3[4], o3["4"]);

// symbol key
const sym = Symbol("tag");
o2[sym] = "sv";
console.log(o2[sym]);

// hasOwnProperty answers expandos; a miss stays a miss
console.log(
  Object.prototype.hasOwnProperty.call(o2, "dynKey"),
  Object.prototype.hasOwnProperty.call(o2, "nope"),
);

// the in operator sees the own expando
console.log("b" in o1, "zzz" in o1);

// expandos on one instance do not leak to another
const o4: any = new C();
console.log(o4.a);
