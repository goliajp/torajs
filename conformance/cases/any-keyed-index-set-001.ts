// cluster #1 blade 4 — recv[key] = v where both sides are `any`: the
// keyed set kernel mirrors the read (ES §7.1.19 ToPropertyKey at
// runtime — numeric lane / Str probe / ToString fallback), the
// (tag, value) pair transfers into the store, Ident receivers ride
// their slot for the dynobj-resize write-back.
var o: any = { a: 1 };
var k: any = "a";
o[k] = 42;
console.log(o.a);

var k2: any = "fresh";
o[k2] = "new";
console.log(o.fresh);

var arr: any = [10, 20, 30];
var ki: any = 1;
arr[ki] = 99;
console.log(arr[1]);

var kd: any = 2.0;
arr[kd] = 77;
console.log(arr[2]);

var kb: any = true;
var o2: any = {};
o2[kb] = "b";
console.log(o2["true"]);

// the motivating end-to-end shape: a this-using HOF callback writing
// back through all-any params
function setter(val, idx, obj) {
  obj[idx] = val * 2;
  return this === undefined;
}
var nums = [1, 2, 3];
nums.every(setter);
console.log(nums);
