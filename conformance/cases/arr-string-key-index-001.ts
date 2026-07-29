// §7.1.19 ToPropertyKey — a string key on an array receiver is an
// element read, a property read, or a miss, decided by its spelling:
// `a["0"]` is the element, `a["length"]` ≡ `a.length`, anything else
// is undefined. The receiver used to reject the whole program; only a
// number key was admitted.
const nums = [10, 20, 30];
console.log(nums["length"]);
console.log(nums["0"]);
console.log(nums["2"]);
console.log(nums["nope"]);
console.log(nums["-1"]);
console.log(nums["1.5"]);

// dynamic string key
let k = "length";
console.log(nums[k]);
k = "1";
console.log(nums[k]);
k = "missing";
console.log(nums[k]);

// a key built at runtime (owned temp — the probe borrows it)
const idx = 2;
console.log(nums["" + idx]);

// string elements
const strs = ["a", "b", "c"];
console.log(strs["length"]);
console.log(strs["1"]);
console.log(strs["zz"]);

// bool elements
const bools = [true, false];
console.log(bools["0"]);
console.log(bools["length"]);

// number keys are unaffected
console.log(nums[0]);
console.log(nums.length);
let n = 1;
console.log(nums[n]);
