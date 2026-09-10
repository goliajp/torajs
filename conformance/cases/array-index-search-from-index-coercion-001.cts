// §23.1.3.14 / §23.1.3.17 / §23.1.3.18 step 4 — fromIndex is
// ToIntegerOrInfinity of whatever was passed: a numeric string counts
// from that index, a non-numeric one is NaN → 0, a boolean is 0 / 1,
// an object runs its valueOf (test262 indexOf/15.4.4.14-5-* and the
// lastIndexOf twins).
var a = [1, 2, 1, 2, 1, 2];
console.log(a.indexOf(2, "2"), a.indexOf(2, "one"), a.lastIndexOf(2, "3"), a.lastIndexOf(2, "x"));
console.log(a.indexOf(1, true), a.indexOf(1, false), a.includes(2, "5"), a.includes(2, "6"));
console.log(a.indexOf(2, { valueOf: function () { return 4; } }), a.lastIndexOf(1, [2]));
