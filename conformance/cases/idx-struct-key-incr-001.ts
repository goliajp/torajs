// keyed update expressions over an any receiver + coercing key
var b: any = {};
b["k"] = 5;
var p = { toString: function(): string { return "k"; } };
console.log(b[p]++);
console.log(b["k"]);
console.log(++b[p]);
console.log(b["k"]);
console.log(b[p]--);
console.log(--b[p]);
console.log(b["k"]);
// coerce count: once per update expression
var n = 0;
var p2 = { toString: function(): string { n++; return "k"; } };
b[p2]++;
console.log(n, b["k"]);
// null receiver throws (catchable)
var c = "";
try { var nl: any = null; nl[p]++; } catch (e) { c = "caught"; }
console.log(c);
// string-ish numeric value steps numerically
b["s"] = "41";
console.log(b[{ toString: function(): string { return "s"; } }]++);
console.log(b["s"]);
