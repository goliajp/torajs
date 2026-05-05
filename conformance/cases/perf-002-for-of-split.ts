// P-iter — `for (let v of <expr>.split(<lit_sep>))` lowers to a
// stack alloca'd SplitIter loop instead of materializing an
// Array<Substr>. Verifies output matches bun across:
//   - happy path
//   - empty tokens (leading / trailing / consecutive seps)
//   - multi-byte separator
//   - empty-sep per-char split
//   - body-uses-`v.length` and `v.charCodeAt(0)` to exercise the
//     borrow-shaped Substr binding through view-aware methods

let total: number = 0;
for (let p of "alpha,beta,gamma,delta".split(",")) {
  total = total + p.length;
}
console.log(total);

let total2: number = 0;
for (let p of ",a,,b,".split(",")) {
  total2 = total2 + p.length;
}
console.log(total2);

let total3: number = 0;
for (let p of "one::two::three".split("::")) {
  total3 = total3 + p.length;
}
console.log(total3);

let total4: number = 0;
for (let p of "abc".split("")) {
  total4 = total4 + p.charCodeAt(0);
}
console.log(total4);

let count: number = 0;
let summed: number = 0;
for (let tok of "1 22 333 4444".split(" ")) {
  count = count + 1;
  summed = summed + tok.length;
}
console.log(count);
console.log(summed);

// Variable-source parent (parent is a let, not a literal).
let s: string = "x|y|z";
let parts_count: number = 0;
for (let q of s.split("|")) {
  parts_count = parts_count + 1;
  console.log(q);
}
console.log(parts_count);
