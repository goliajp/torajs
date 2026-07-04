// L3b #13 (chunk 528) — string-key indexing on any receivers per
// ES ToPropertyKey: a literal key rides the full member path
// (o["k"] === o.k, including non-identifier names like "7" and
// "a b"), and a dynamic string key probes/stores by its runtime
// value. Reads of absent keys answer undefined.
const o: any = { a: 1, "7": "seven", "a b": "spaced" };
console.log(o["a"]);
console.log(o["7"]);
console.log(o["a b"]);
console.log(o["missing"]);
o["7"] = "SEVEN";
console.log(o[7]);
o["x y"] = 42;
console.log(o["x y"]);
let k = "a";
console.log(o[k]);
k = "a b";
console.log(o[k]);
const parts = ["x", "y"];
const dynkey = parts.join(" ");
console.log(o[dynkey]);
o[dynkey] = 43;
console.log(o["x y"]);
const grow: any = {};
for (let i = 0; i < 12; i++) {
  grow["k" + i] = i;
}
console.log(grow["k0"]);
console.log(grow["k11"]);
