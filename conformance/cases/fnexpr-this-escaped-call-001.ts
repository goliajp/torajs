// a promoted fn-expr face read back out of its descriptor and called
// directly: this is undefined (strict §10.2.1.2), args do not shift
// (the any-lane invoke reads FLAG_CLOSURE_RECV_FIRST and slots the
// receiver channel separately)
const o: any = {};
Object.defineProperty(o, "y", {
  get: function () { return this === undefined ? "this-undef" : "this-recv"; },
});
const d: any = Object.getOwnPropertyDescriptor(o, "y");
const g: any = d.get;
console.log(typeof g);
console.log(g());

const p: any = {};
Object.defineProperty(p, "z", {
  set: function (v: any) { console.log("setter got:", v, "this:", this === undefined ? "undef" : "obj"); },
});
const s: any = Object.getOwnPropertyDescriptor(p, "z").set;
s(42);
