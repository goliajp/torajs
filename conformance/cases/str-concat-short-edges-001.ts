// Short-operand concat at its edges: an empty accumulator taking a wide
// and a high Latin-1 literal, a lone surrogate literal by value, the
// 64-byte pooled class boundary for both encodings (in place, then one
// past), and an accumulator whose view is still held while it grows.
// Acceptance: byte-equal with bun.

let e = "";
e = e + "中";
e = e + "é";
console.log(e.length, e.charCodeAt(0), e.charCodeAt(1));

const lone = "x" + "\uD800";
console.log(lone.length, lone.charCodeAt(1));

let a = "q".repeat(57);
a = a + "1234567";
console.log(a.length, a.slice(56));
a = a + "8";
console.log(a.length, a.slice(56));

let w = "中".repeat(30);
w = w + "文字测试";
console.log(w.length, w.charCodeAt(33));
w = w + "字";
console.log(w.length, w.charCodeAt(34));

let s = "ab,cd".repeat(2);
const v = s.split(",")[1];
s = s + v;
s = s + ";";
console.log(s, v);

let z = "";
z = z + "";
z = "" + z;
console.log(z.length, JSON.stringify(z + "x" + ""));
