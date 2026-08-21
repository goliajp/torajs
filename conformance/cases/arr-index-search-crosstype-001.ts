// ES §7.2.15 strict equality and §7.2.10 SameValueZero compare by TYPE
// first — a needle of another type is a question with an answer, not a
// type error.
const nums = [1, 2, 3, NaN, 0];
console.log(nums.includes("42"), nums.indexOf("2"), nums.lastIndexOf("3"));
console.log(nums.includes([42] as any), nums.includes(42.0), nums.includes(NaN));
console.log(nums.indexOf(true as any), nums.includes(false as any));
console.log(nums.indexOf(null as any), nums.indexOf(undefined as any));
console.log(nums.includes(-0), nums.indexOf(0));

const strs = ["a", "b", "c"];
console.log(strs.includes(1 as any), strs.indexOf(2 as any), strs.lastIndexOf(3 as any));
console.log(strs.includes("b"), strs.indexOf("c"), strs.indexOf("b", "1" as any));

// A fresh-owned needle is boxed for the by-tag compare; it must be
// released exactly once.
const k = "4" + "2";
console.log(nums.includes(k as any), nums.indexOf(k as any));

// The needle still evaluates exactly once.
let hits = 0;
const once = { valueOf() { hits++; return 2; } };
console.log(nums.indexOf(once as any), hits);
