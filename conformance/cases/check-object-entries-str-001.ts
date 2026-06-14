// W-O-3-str — Object.entries(str) returns [[idx_str, char_str], ...]
// Spec ES §22.1.5.2 + §20.1.2.5: ToObject on a primitive string +
// own-keys walk enumerates the per-char indexed view as
// [["0", s[0]], ["1", s[1]], ...]. tora loops __torajs_str_at to
// mint fresh Strs per code unit (same materialize as W-O-2 / W-M-rest).
// 3 shapes via typed-var indexed access (see W-O-3-arr fixture for
// the L3b W-O-3-nested-print rationale).

const a = "hi";
const ea = Object.entries(a);
console.log(ea.length);
const ea0 = ea[0];
const ea1 = ea[1];
console.log(ea0[0]);
console.log(ea0[1]);
console.log(ea1[0]);
console.log(ea1[1]);

const b = "";
const eb = Object.entries(b);
console.log(eb.length);

const c = "x";
const ec = Object.entries(c);
console.log(ec.length);
const ec0 = ec[0];
console.log(ec0[0]);
console.log(ec0[1]);
