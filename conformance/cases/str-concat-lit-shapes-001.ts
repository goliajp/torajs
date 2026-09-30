// String concat with a short literal operand: a separator that changes
// inside the loop is not a literal, eight-byte literals with the top bit
// set, a lone surrogate literal, template static parts, prepend /
// compound / two adjacent literals, and appends past every pooled size.
// Acceptance: byte-equal with bun.

let s = "", sep = "";
for (let i = 0; i < 4; i++) { s = s + sep + i; sep = ","; }
console.log(s);

let a = "ab".repeat(2);
console.log(a + "ÿÿÿÿÿÿÿÿ", "ÿÿÿÿÿÿÿÿ" + a);
for (let i = 0; i < 3; i++) a = a + "ÿÿÿÿÿÿÿÿ";
console.log(a.length, a.charCodeAt(a.length - 1));

const p = "a,b".split(",");

const x = p[0];
console.log((x + "\uD83D").charCodeAt(1), `${x},${p[1]};`, `<${x}>`);

let t = "z";
for (let i = 0; i < 3; i++) { t = "<" + t; t += ">"; t = t + "a" + "b"; }
console.log(t);

let big = "";
for (let i = 0; i < 200000; i++) big = big + "xy";
console.log(big.length, big.charCodeAt(399999));

const m = new Map<string, number>();
m.set(x + "k", 1);
console.log(m.get("a" + "k"), x + "k" === "ak", (x + "中").length, ("中" + x + "é").charCodeAt(2));
