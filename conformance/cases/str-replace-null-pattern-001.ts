// RC-4 replace A1_T4 — ToString(searchValue) for null / undefined
// literal patterns (ES §22.1.3.18 step 3): `replace(null, ...)`
// previously lowered the pattern to a null ptr in the helper's Str
// param and the runtime deref'd it (SIGSEGV). The replace family's
// checker signature is (Any, Any) so the literals arrive here; the
// lowering folds them to their ToString text.

// null pattern + string replacement
console.log("anullb".replace(null, "X"));

// null pattern + functional replacement (the test262 A1_T4 shape)
console.log("gnulluna".replace(null, function (a1, a2, a3) { return a2 + ""; }));

// undefined pattern + string replacement
console.log("xundefinedy".replace(undefined, "Z"));

// null in the replacement slot takes ToString too
console.log("ab".replace("a", null));

// replaceAll with a null pattern walks every occurrence
console.log("null-null".replaceAll(null, "N"));

// no match: pattern text absent
console.log("abc".replace(null, "X"));
