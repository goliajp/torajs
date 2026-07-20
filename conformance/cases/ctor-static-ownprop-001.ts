// ctor cell any-lane own-property 反射 (RFC 20260721-object-descriptor-
// cluster 刀 3) — hasOwnProperty / propertyIsEnumerable / dynamic-key
// member reads over a builtin ctor cell agree with the gOPD surface:
// table statics, `prototype`, and Number's §21.1.2 data constants are
// own properties (statics enumerable: false).
const o: any = Object;
console.log(o.hasOwnProperty("keys"), o.hasOwnProperty("nosuch"));
console.log(o.hasOwnProperty("prototype"), o.hasOwnProperty("getPrototypeOf"));
console.log(o.propertyIsEnumerable("keys"), o.propertyIsEnumerable("nosuch"));
console.log(o["keys"] === Object.keys);
console.log(o["prototype"] === Object.prototype);

const d: any = Date;
console.log(d.hasOwnProperty("now"), d.hasOwnProperty("parse"));
const k = "now";
console.log(d[k] === Date.now);

const n: any = Number;
console.log(n.hasOwnProperty("MAX_VALUE"), n.hasOwnProperty("MIN_SAFE_INTEGER"));
console.log(n["MAX_VALUE"], n["MAX_SAFE_INTEGER"]);
console.log(n.propertyIsEnumerable("MAX_VALUE"));

const s: any = String;
console.log(s.hasOwnProperty("fromCharCode"), s["fromCharCode"](72, 105));
