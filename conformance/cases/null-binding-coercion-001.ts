// Six coercion sites each asked `operand == ConstPtrNull`, which only
// ever holds for the LITERAL. Bind the same value and the read is a
// Load, so every one of them fell off its own arm: three panicked
// ("cannot coerce Ptr to f64" / "to i64 for bitwise" / "String() with
// arg type Ptr"), `+s` answered the pointer as `null`, `-s` lost the
// sign, and `"x" + s` printed the pointer's decimal address.
const s = null;

console.log(+s, -s, ~s, !s);
console.log(s | 0, s & 7, s ^ 3, s << 2, s >> 1, s >>> 0);
console.log(0 | s, 7 & s, 3 ^ s);
console.log(s + 1, s - 1, s * 3, s / 2, s % 2);
console.log(String(s), `${s}`, "x" + s, s + "x");
console.log(Number(s), Boolean(s), s ?? 7);

let reassigned = null;
reassigned = null;
console.log(+reassigned, ~reassigned, String(reassigned));

const holder = { v: null };
console.log(+holder.v, ~holder.v, String(holder.v));

// A nullable pointer keeps its runtime branch, and the literal keeps
// answering what it always did.
let maybe: string | null = "ab";
console.log("x" + maybe);
maybe = null;
console.log("x" + maybe);
console.log(+null, -null, ~null, null | 0, null + 1, String(null));

// Nothing else moved onto the null arms.
console.log(+true, ~true, +"ab", ~5.9, "x" + undefined, "x" + { a: 1 });
