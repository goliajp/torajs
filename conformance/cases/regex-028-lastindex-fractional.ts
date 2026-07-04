// L3b #12 (chunk 526) — lastIndex is an ordinary data property:
// assignment stores the value uncoerced (f64 slot), so fractional
// writes read back exactly; ToLength happens only where exec /
// test / match consume it (2.9 starts the scan at index 2).
const r = /ab/g;
r.lastIndex = 2.9;
console.log(r.lastIndex);
const m = r.exec("ababab");
console.log(m[0]);
console.log(r.lastIndex);
const s = /ab/y;
s.lastIndex = 1.5;
console.log(s.test("xab"));
console.log(s.lastIndex);
s.lastIndex = 0.5;
console.log(s.test("ab"));
const t = /x/;
t.lastIndex = 7.25;
console.log(t.lastIndex);
console.log(t.test("x"));
console.log(t.lastIndex);
const u: any = /cd/g;
u.lastIndex = 1.75;
console.log(u.lastIndex);
console.log(u.test("cdcd"));
console.log(u.lastIndex);
const v = /q/g;
v.lastIndex = 3;
console.log(v.lastIndex);
