// §23.1.3 step 1 ToObject — generic Array read methods over
// primitive-wrapper and primitive receivers: the callback's O is the
// wrapper object (obj instanceof String/Number/Boolean), a String
// wrapper scans its [[StringData]] characters + expando, and a fresh
// bool/number wrapper has length 0 (vacuous scan).
function isStr(val, idx, o) { return o instanceof String; }
var s: any = new String("ab");
console.log(Array.prototype.every.call(s, isStr));
console.log(Array.prototype.map.call(s, function (c) { return c + "!"; }));
console.log(Array.prototype.indexOf.call(s, "b"));

// string primitive: O is the minted wrapper
console.log(Array.prototype.every.call("xy", isStr));
console.log(Array.prototype.filter.call("xyz", function (c) { return c !== "y"; }));

// expando past the character face
var t: any = new String("q");
t[5] = "far";
console.log(t.length, Array.prototype.indexOf.call(t, "far"));

// number / boolean wrapper with expando length + indices
function isNum(val, idx, o) { return o instanceof Number; }
var n: any = new Number(5);
n.length = 2; n[0] = 7; n[1] = 8;
console.log(Array.prototype.every.call(n, isNum));
console.log(Array.prototype.map.call(n, function (v) { return v * 10; }));

// bare primitives: vacuous scans
console.log(Array.prototype.every.call(false, function () { return false; }));
console.log(Array.prototype.map.call(2.5, function (v) { return v; }));
