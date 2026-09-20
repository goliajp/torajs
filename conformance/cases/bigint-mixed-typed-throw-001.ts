// §13.15.3 does ToNumeric on both operands and throws a TypeError when
// the resulting types disagree. With both operand types known while
// compiling, that outcome is decided there — the operands still
// evaluate (the spec evaluates them before it coerces, so their side
// effects are observable), and then the throw happens, with no runtime
// type dispatch to reach a conclusion already reached.
//
// The point of the fixture is that the program RUNS. A type checker
// that only reported would produce nothing to run; refusing the
// program would reject something the spec defines completely.
//
// Only the error CLASS is compared: §13.15.3 mandates TypeError and
// says nothing about the message, and the engines word it differently.

let caught: string = "none";
try {
    let n: bigint = 1n;
    let m: number = 1;
    console.log(n & m);
} catch (e: any) {
    caught = e.constructor.name;
}
console.log("and:", caught);

// Operand side effects happen before the throw.
let steps: string = "";
function l(): bigint {
    steps = steps + "L";
    return 2n;
}
function r(): number {
    steps = steps + "R";
    return 3;
}
try {
    console.log(l() - r());
} catch (e: any) {
    steps = steps + "|" + e.constructor.name;
}
console.log("order:", steps);

// A throw crossing a function boundary is still catchable — the
// may-throw fixpoint has to know about it or the caller's check is
// pruned and the throw is swallowed.
function boom(a: bigint, b: number): bigint {
    return a * b;
}
try {
    boom(4n, 5);
    console.log("not reached");
} catch (e: any) {
    console.log("crossed:", e.constructor.name);
}

// `>>>` has no BigInt form at all (§13.12), so it throws even when
// both sides are BigInt.
try {
    let x: bigint = 8n;
    console.log(x >>> 1n);
} catch (e: any) {
    console.log("ushr:", e.constructor.name);
}

// Same-typed operands are untouched by any of this.
let a: bigint = 7n;
let b: bigint = 2n;
console.log("bigint pair:", a + b, a * b, a & b);
console.log("number pair:", 7 + 2, 7 & 2);
