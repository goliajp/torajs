// chunk B1 (RFC 20260711 for-in) — Object.keys / getOwnPropertyNames
// ES §20.1.2.17 ToObject dispatch on any receivers: primitives answer
// their wrapper's own enumerable keys instead of the former loud
// non-struct TypeError.
const n: any = 42;
console.log(Object.keys(n).length);
const b: any = true;
console.log(Object.keys(b).length);
const s: any = "ab";
console.log(Object.keys(s).join(","));
const sl: any = "abcdefgh";
console.log(Object.keys(sl).join(","));
console.log(Object.getOwnPropertyNames(sl).join(","));
const a: any = [10, 20];
console.log(Object.keys(a).join(","));
a.x = 5;
console.log(Object.keys(a).join(","));
console.log(Object.getOwnPropertyNames(a).join(","));
const f: any = () => 1;
f.y = 7;
console.log(Object.keys(f).join(","));
try {
  Object.keys(null as any);
} catch (e) {
  console.log("null throws", e instanceof TypeError);
}
try {
  Object.keys(undefined as any);
} catch (e) {
  console.log("undef throws", e instanceof TypeError);
}
console.log("done");
