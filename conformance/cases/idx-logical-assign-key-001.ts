var count = 0;
var obj = {};
function incr(): number { return ++count; }
console.log((obj[incr()] &&= incr()), obj[1], count);   // undefined undefined 1
obj[2] = 1;
console.log((obj[incr()] &&= incr()), obj[2], count);   // 3 3 3
var o2: any = {};
console.log((o2[incr()] ||= incr()), o2[4], count);     // 5 5 5
console.log((o2[incr()] ??= incr()), o2[6], count);     // 7 7 7
console.log((o2[incr()] ??= 99), o2[6], count);         // 99 7 8  (o2[8] 设了 99)
// coercing key + logical: toString once per expression
var n = 0;
var kp = { toString: function(): string { n++; return "kk"; } };
var o3: any = {};
console.log((o3[kp] ??= 42), n);                        // 42 1
console.log((o3[kp] ||= 7), n, o3["kk"]);               // 42 2 42
