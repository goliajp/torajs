// §22.1.3.13 String.prototype.match step 3 — custom @@match dispatch
// on an Any-typed pattern: GetMethod(regexp, @@match) then
// Call(matcher, regexp, «S»); absent/nullish matcher falls back to
// the step-4 RegExpCreate coerce lane; a present-but-not-callable
// matcher is the GetMethod step-4 TypeError.

// custom matcher: called once, this = pattern obj, arg = receiver S
var obj = {};
var callCount = 0;
obj[Symbol.match] = function (s: string) {
  callCount += 1;
  const self: any = this;
  return { hit: true, isSelf: self === obj, arg: s };
};
const r: any = "".match(obj);
console.log(r.hit, r.isSelf, r.arg === "", callCount);

// matcher answering a non-object value
var m2 = {};
m2[Symbol.match] = function (s: string) {
  return 42;
};
console.log("abc".match(m2));

// null matcher -> step 4 coerce through toString (GetMethod step 3)
var m3: any = {};
m3[Symbol.match] = null;
m3.toString = function () {
  return "\\d";
};
const r3: any = "ab3c".match(m3);
console.log(r3[0]);
console.log("abc".match(m3));

// non-callable matcher -> TypeError (GetMethod step 4)
var m4: any = {};
m4[Symbol.match] = 5;
try {
  "x".match(m4);
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}

// no @@match at all -> coerce lane
var m5: any = {};
m5.toString = function () {
  return "b";
};
const r5: any = "abc".match(m5);
console.log(r5[0]);
