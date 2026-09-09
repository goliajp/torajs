// §13.6-§13.10 call ToNumeric on both operands unconditionally, and
// ToNumeric on a String is ToNumber. The arm that says so already
// existed for the bitwise six (`string_numeric_pair`) but was held
// back from the arithmetic family, which inherited the exclusion
// through `js_arith_coerces_to_number` — a verbatim copy of the `+`
// rule, where a String side means CONCAT and so must stay out.
// `- * / %` and `**` have no concat path; every one of these ran
// already when the same value was spelled `any`.

const s: string = "2";
const t: string = "3";
const n: number = 3;

console.log(s * n, n - s, s / t, s % 4, s ** n);
console.log("abc" * 2, "" * 5, "  7  " - 0);
console.log(s - null, s - true, s - undefined);
console.log(1 - s, 4 % s, 2 ** s);

// ordering — §7.2.13 takes the lexicographic branch only when BOTH
// sides are Strings; past it, ToNumeric on each.
console.log(s < n, n > s, s < true, s >= null);
console.log(s <= "10", t > "10");

// the bitwise six kept their own arm; `undefined` joins the set
console.log(s | 0, s & 3, s ^ 1, "6" | undefined);
