// ES §19.2.{3,4} step 1 — global isFinite / isNaN apply `? ToNumber`
// to the arg; an abrupt completion from an Any-boxed dynobj's
// valueOf / toString must propagate. Regression fix: the Any arm in
// lower_is_nan_or_finite called any_to_number without an
// emit_throw_check → the pending exception stashed while a NaN
// garbage value continued into the predicate + downstream drops →
// SIGSEGV (test262 isFinite / isNaN `return-abrupt-from-tonumber-
// number.js` both exit 139).

var obj1: any = { valueOf: function() { throw new Error("VOF"); } };
var obj2: any = { toString: function() { throw new Error("STR"); } };

// isFinite
try { isFinite(obj1); console.log("isFinite obj1: no throw (BAD)"); }
catch (e: any) { console.log("isFinite obj1 throws:", e.message); }
try { isFinite(obj2); console.log("isFinite obj2: no throw (BAD)"); }
catch (e: any) { console.log("isFinite obj2 throws:", e.message); }

// isNaN
try { isNaN(obj1); console.log("isNaN obj1: no throw (BAD)"); }
catch (e: any) { console.log("isNaN obj1 throws:", e.message); }
try { isNaN(obj2); console.log("isNaN obj2: no throw (BAD)"); }
catch (e: any) { console.log("isNaN obj2 throws:", e.message); }

// Positive lanes still work (regression guard).
console.log("isFinite(1/0):", isFinite(1 / 0));
console.log("isNaN(NaN):", isNaN(NaN));
console.log("isFinite(42):", isFinite(42));
console.log("isNaN(0):", isNaN(0));

// Any-boxed number stays correct (S343 guard).
var a: any = 42;
console.log("isFinite(any 42):", isFinite(a));
console.log("isNaN(any 42):", isNaN(a));
