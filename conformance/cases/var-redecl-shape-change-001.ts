// cluster #11 (rotation 442): §14.3.2 shares one binding across every
// same-name `var`; a re-declaration with a different shape lands in
// the Any prelude slot, not the first shape's pinned let.
var o = { a: 1 };
var o = { b: "x" };
console.log(o.b);
var q = { valueOf: function() { return {}; }, toString: function() { return {}; } };
console.log(typeof q);
var q = { valueOf: function() { return 1; } };
console.log(q.valueOf());
var r = [1, 2];
var r = "now-a-string";
console.log(r, r.length);
try { var t = { x: 1 }; } catch (e) {}
var t = { y: 2 };
console.log(t.y);
// single-declaration names keep their typed escape (regression face)
var solo = [7, 8, 9];
console.log(solo.length, solo[2]);
