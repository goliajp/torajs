// RFC 20260705 chunk 557 — the return-ann sniff's Member-call
// whitelist had no conversion-method rows: `j.toString()` inside a
// string-concat chain bailed infer_expr_ann_with → the whole arrow
// kept the Void return default → "return type mismatch: function
// expects Void, got String" at every constrained use (loud).
let words: string[] = [];
for (let j = 0; j < 2; j++) {
  let tag = (s: string) => s + "-" + j.toString();
  words.push(tag("w"));
}
console.log(words[0]);
console.log(words[1]);

let nums: string[] = [];
for (let k = 0; k < 2; k++) {
  let fmt = (x: number) => x.toFixed(2) + ":" + k.toString();
  nums.push(fmt(1.5));
}
console.log(nums[0]);
console.log(nums[1]);

// number toPrecision / toExponential through the same lane.
let sci = (x: number) => x.toExponential(1) + "|" + x.toPrecision(3);
console.log(sci(1234.5));

// boolean receiver.
let flag = true;
let show = (s: string) => s + "=" + flag.toString();
console.log(show("on"));

// array receiver (elem-typed) toString.
let xs: number[] = [1, 2, 3];
let dump = (p: string) => p + "[" + xs.toString() + "]";
console.log(dump("arr"));

// string receiver toString (identity-ish but exercises the row).
let base = "core";
let echo = (n: number) => base.toString() + "#" + n.toString();
console.log(echo(9));
