// chunk 745 — struct receiver + compile-time literal index:
// `g[0]` ≡ `g."0"` per ES ToPropertyKey (§7.1.19). Numeric-key
// object literals store fields under their integer spelling
// (P0.10); a literal index resolves statically through the member
// lanes on both read and write. Non-identifier string keys
// (`g["a-b"]`) ride the same lane.
const g = { 0: "raw", 5: "five" };
console.log(g[0], g[5]);
console.log(g["0"]);
g[0] = "changed";
console.log(g[0]);
const h = { a: 1, 0: "zero" };
console.log(h[0], h.a);
const nums = { 0: 10, 5: 50 };
console.log(nums[0] + nums[5]);
nums[5] = 90;
console.log(nums[5] - nums[0]);
const dash = { "a-b": 7 };
console.log(dash["a-b"]);
