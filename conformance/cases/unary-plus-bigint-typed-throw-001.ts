// §7.1.4 ToNumber refuses a BigInt outright, and unary `+` is nothing
// but ToNumber (§13.5.4). With the operand's type known at compile
// time the outcome is known too, so the throw is emitted rather than
// re-derived at run time.
//
// The rejection this replaces described itself: it read "`+` on bigint
// is a TypeError per spec" — a compile-time refusal of a program the
// spec defines completely.

let b: bigint = 1n;
try {
    console.log(+b);
} catch (e: any) {
    console.log("unary plus:", e.constructor.name);
}

// The operand still evaluates before the throw.
let ran: string = "";
function make(): bigint {
    ran = "yes";
    return 2n;
}
try {
    console.log(+make());
} catch (e: any) {
    console.log("operand ran:", ran, "then", e.constructor.name);
}

// Across a function boundary the throw stays catchable — the may-throw
// fixpoint has to know, or the caller's check is pruned.
function neg(x: bigint): number {
    return +x;
}
try {
    neg(3n);
    console.log("not reached");
} catch (e: any) {
    console.log("crossed:", e.constructor.name);
}

// The other unary operators on a BigInt are untouched: §13.5.5 negate
// and §13.5.7 bitwise NOT both have BigInt forms.
console.log("neg:", -b, "not:", ~b);
console.log("number plus:", +(2 as number));
