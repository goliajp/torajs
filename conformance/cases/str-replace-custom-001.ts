// §22.1.3.19 String.prototype.replace step 3 — custom @@replace
// dispatch on an Any-typed searchValue: Call(replacer, searchValue,
// «O, replaceValue»); absent/nullish replacer falls back to
// ToString(searchValue) + the LITERAL substring kernels (step 4
// never mints a RegExp).

// custom replacer: this = searchValue, args = (S, replaceValue)
var sv = {};
var callCount = 0;
sv[Symbol.replace] = function (s: string, r: string) {
  callCount += 1;
  const self: any = this;
  return "custom:" + (self === sv) + ":" + s + ":" + r;
};
console.log("ab3c".replace(sv, "X"));
console.log(callCount);

// fn replaceValue boxes through the same «O, replaceValue» slot
var sv1 = {};
sv1[Symbol.replace] = function (s: string, r: any) {
  return "got:" + typeof r;
};
console.log("x".replace(sv1, function (m: string) {
  return m;
}));

// null @@replace -> literal-substring fallback, both replacer shapes
var sv2 = {};
sv2[Symbol.replace] = null;
sv2.toString = function () {
  return "3";
};
console.log("ab3c".replace(sv2, "<foo>"));
console.log("ab3c".replace(sv2, function (m: string) {
  return "<" + m + ">";
}));

// non-callable @@replace -> TypeError
var sv3 = {};
sv3[Symbol.replace] = 5;
try {
  "x".replace(sv3, "y");
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}

// no @@replace at all -> fallback through toString
var sv4 = {};
sv4.toString = function () {
  return "b";
};
console.log("abc".replace(sv4, "Z"));

// single-arg spelling: replaceValue = undefined; @@replace still probed
var sv5 = {};
sv5[Symbol.replace] = function (s: string, r: any) {
  return "one:" + (r === undefined);
};
console.log("q".replace(sv5));
