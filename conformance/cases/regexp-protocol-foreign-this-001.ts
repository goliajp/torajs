// A RegExp protocol method read off a RegExp and called on another object
// runs the spec algorithm over that object (§22.2.6.8 / .11 / .12): its
// RegExpExec calls the object's own `exec`, and an object without one is a
// TypeError — whether the method sits in an own property, an inherited one,
// a symbol-keyed slot, or is reached through `.call`.
const re: any = /a/;
const o: any = { m: re[Symbol.match] };
try {
  console.log(o.m("a"));
} catch (e) {
  console.log("own no exec", e instanceof TypeError);
}
const o2: any = { m: re[Symbol.match], exec(s: string) { return null; } };
console.log(o2.m("a"));
const o3: any = { m: re[Symbol.replace], exec(s: string) { return null; } };
console.log(o3.m("abc", "x"));
const o5: any = { exec(s: string) { return { 0: "zz", length: 1, index: 1 }; } };
o5.m = re[Symbol.search];
console.log(o5.m("abc"));
const base: any = { m: re[Symbol.match] };
const child: any = Object.create(base);
try {
  console.log(child.m("a"));
} catch (e) {
  console.log("inherited no exec", e instanceof TypeError);
}
const o6: any = { exec(s: string) { return null; } };
o6[Symbol.match] = re[Symbol.match];
console.log(o6[Symbol.match]("a"));
const f: any = re[Symbol.search];
try {
  console.log(f.call(1, "a"));
} catch (e) {
  console.log("primitive this", e instanceof TypeError);
}
console.log(f.call(/b/, "abc"));
