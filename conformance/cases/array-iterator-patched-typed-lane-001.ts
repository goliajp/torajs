// §8.6.2 / §14.7.5.6 — destructuring and for-of over a statically
// typed Array still go through Array.prototype[Symbol.iterator], so a
// module that replaces it observes the replacement on typed sources
// too (the compile-time stand-down in builtin_proto_shadow).
(Array.prototype as any)[Symbol.iterator] = function* (this: number[]) {
  for (let i = this.length - 1; i >= 0; i--) yield this[i] * 10;
};
const nums: number[] = [1, 2, 3];
const [a, b] = nums;
console.log("dstr", a, b);
const seen: number[] = [];
for (const v of nums) seen.push(v);
console.log("forof", seen.join(","));
const [first, ...rest] = nums;
console.log("rest", first, rest.join(","));
function take([x, y]: number[]): string {
  return x + "/" + y;
}
console.log("param", take(nums));
const strs: string[] = ["p", "q"];
const [s1, s2] = strs;
console.log("strings", s1, s2);
let m: number, n: number;
[m, n] = nums;
console.log("assign", m, n);
console.log("index untouched", nums[0], nums.length);
