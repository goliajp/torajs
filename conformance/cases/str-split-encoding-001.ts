// RFC 20260711 follow-up — String.split on the code-unit grid (the
// byte-based shape wrote byte offsets into the unit-semantics Substr
// fields, garbling every UTF-16 haystack split).
const parts = "汉x字x界".split("x");
console.log(parts.length, parts[0], parts[1], parts[2]);
console.log(parts[0].length, parts[1].length, parts[2].length);
console.log(parts.join("-"));
console.log("中文界".split(""));
console.log("a中b".split("中"));
console.log("汉字".split("z"));
console.log("汉、字、界".split("、"));
console.log("a,b".split(","));
console.log("𝄞x𝄢".split("x"));
let acc = "";
for (const ch of "汉x字".split("x")) acc += ch + ".";
console.log(acc);
