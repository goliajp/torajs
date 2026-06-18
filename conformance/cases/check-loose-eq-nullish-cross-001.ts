// ES §7.2.13 IsLooselyEqual — `null` and `undefined` never coerce
// to other primitive types; the cross-type loose-eq result is
// always false. The corresponding `!=` flips to true. Same-side
// nullish stays true (covered by the prior commit).
//
// Pre-fix tr type-rejected at check time with "strict equality
// requires same primitive type" for every Null/Undefined cross-
// primitive pair, even though SSA already had the spec-correct
// `ConstBool` fold (a_is_null/b_is_null branch). This commit
// teaches the check.rs nullish guard to let those pairs through.
console.log(null == 0);            // false
console.log(undefined == 0);       // false
console.log(null == "");           // false
console.log(undefined == "");      // false
console.log(null == false);        // false
console.log(undefined == false);   // false
console.log(null == "x");          // false
console.log(undefined == "x");     // false
console.log(null == true);         // false
console.log(undefined == true);    // false

// `!=` flip
console.log(null != 0);            // true
console.log(undefined != "x");     // true
console.log(undefined != false);   // true

// Reverse operand order — symmetric per spec
console.log(0 == null);            // false
console.log("" == undefined);      // false
console.log(false == null);        // false
