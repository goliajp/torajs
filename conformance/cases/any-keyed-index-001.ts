// cluster #1 blade 3 — recv[key] where BOTH sides are `any`: the
// runtime keyed kernel does the ES §7.1.19 ToPropertyKey dispatch
// (numeric lane / Str probe / ToString fallback for bool, null,
// non-integral doubles). The motivating shape is a promoted
// callback's all-any body reading obj[idx].
var o: any = { a: 1, "2": "two" };
var k1: any = "a";
var k2: any = 2;
var k3: any = 2.0;
console.log(o[k1], o[k2], o[k3]);

var arr: any = [10, 20, 30];
var ki: any = 1;
var ks: any = "length";
console.log(arr[ki], arr[ks]);

var kd: any = 1.5;
var o2: any = { "1.5": "frac" };
console.log(o2[kd]);

var kb: any = true;
var o3: any = { "true": "b" };
console.log(o3[kb]);

var s: any = "hello";
var k5: any = 1;
console.log(s[k5]);

var kn: any = null;
var o4: any = { "null": "n" };
console.log(o4[kn]);

// the motivating end-to-end shape: this-using named fn as HOF
// callback, body reads obj[idx] through all-any params
var nums = [10, 20, 30];
function cb(val, idx, obj) {
  return this === undefined && obj[idx] === val;
}
console.log(nums.every(cb));

// blade 5 — a typed struct receiver with an `any` key boxes at the
// lane boundary and rides the same keyed kernel
type P = { a: number, b: string };
const p: P = { a: 7, b: "x" };
var pk: any = "a";
console.log(p[pk]);
var pk2: any = "missing";
console.log(p[pk2]);
