// §22.1.3.20 String.prototype.search step 3 — custom @@search
// dispatch on an Any-typed pattern; store-free Any patterns take
// the step-4 RegExpCreate coerce lane (regex semantics, Number).

// custom searcher: this = pattern obj, arg = receiver S
var obj = {};
var callCount = 0;
obj[Symbol.search] = function (s: string) {
  callCount += 1;
  const self: any = this;
  return { hit: self === obj, arg: s };
};
const r: any = "abc".search(obj);
console.log(r.hit, r.arg, callCount);

// store-free Any pattern -> coerce lane, regex (not indexOf) semantics
var pat = ".";
console.log("a.c".search(pat));

// null @@search -> coerce through toString
var s3: any = {};
s3[Symbol.search] = null;
s3.toString = function () {
  return "b";
};
console.log("abc".search(s3));

// non-callable @@search -> TypeError
var s4: any = {};
s4[Symbol.search] = 7;
try {
  "x".search(s4);
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
