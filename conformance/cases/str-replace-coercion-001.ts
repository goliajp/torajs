// String.prototype.replace / replaceAll ToString coercion of
// non-regex searchValue and non-fn replaceValue (ES 22.1.3.19
// steps 3/6) -- user toString hooks run, throws propagate in
// argument order (searchValue first), and fresh-owned argv
// temps drop after the helper call (leak regression face).
// RFC 20260712-string-proto-cluster chunk B.

// object searchValue coerces through its toString.
var sv: any = { toString: function() { return "B"; } };
console.log("ABBA".replace(sv, "x"));

// throwing searchValue toString propagates (before replaceValue).
var t1: any = { toString: function() { throw "insearch"; } };
var t2: any = { toString: function() { throw "inreplace"; } };
try { "ABBA".replace(t1, t2); console.log("no-throw"); } catch (e) { console.log("caught:" + e); }

// non-fn replaceValue coerces too.
var rv: any = { toString: function() { return "Z"; } };
console.log("ABBA".replace("B", rv));
try { "ABBA".replace("B", t2); console.log("no-throw"); } catch (e) { console.log("caught:" + e); }

// numeric searchValue.
console.log("1423".replace(42 as any, "x"));
console.log("x1y".replace(1 as any, "#"));

// replaceAll with an object searchValue.
console.log("a-b-a".replaceAll("a", { toString: function() { return "c"; } } as any));

// fresh-owned temp args drop after the call (leak face; churn
// probe validates RSS, this validates behavior).
const n = "xq";
console.log("axb".replace(n.slice(0, 1), "z"));
console.log("axb".replace(("x" + "q").slice(0, 1), "z"));
