// RC-2 (RFC 20260706-test262-bug-corpus): s.match(re) through an any
// receiver — the Tag::Str arm routes RegExp-cell args onto the typed
// tier's match kernel; the product is heap-chain-marked so any-tier
// index reads box the Str cells, and a missed optional capture reads
// as undefined (not a boxed null pointer).
var s = "Boston, MA 02134";
var m = s.match(/([\d]{5})([-\ ]?[\d]{4})?$/);
console.log(m.length);
console.log(m[0], m[1], m[2]);
console.log(s.match(/zzz/));
var s2 = "Boston";
console.log(s2.match(/os/)[0]);
