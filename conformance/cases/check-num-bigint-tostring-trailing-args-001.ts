// S294 — Number/BigInt radix-method + Number.{toLocaleString,toFixed,
// toExponential,toPrecision}(...trailing) trailing-arg ignore per ES
// §21.1.3.6 (Number.toString), §21.1.3.4 (Number.toLocaleString),
// §21.1.3.{3,5,6} (toFixed/toExp/toPrec), §21.2.3.5 (BigInt.toString).
// Spec reads only the first useful slot (radix / digits / locale);
// tora's SSA-emit `args.first()` lower + helper Call left trailing
// args[1..] silently dropped (probe shows calls=0 vs bun's natural
// accumulation). check.rs S244/S247/S254/S260 typecheck-and-drops;
// ssa_lower now lower-and-drops in 5 sites: BigInt toString undef
// short-circuit, BigInt toString radix, Number toString i64 radix,
// Number toString f64 radix, Number toLocaleString/toFixed/toExp/
// toPrec arm. step()-side-effect exprs fire per ES eval-then-discard
// (S272 idiom).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const n: number = 42;
console.log("n.toString radix trail:", n.toString(10, step("nt1")));
console.log("calls:", calls);
console.log("n.toString radix multi:", n.toString(16, step("nt2"), step("nt3")));
console.log("calls:", calls);
console.log("n.toFixed bare:", n.toFixed(2));
console.log("n.toFixed trail:", n.toFixed(2, step("nf1")));
console.log("n.toFixed multi:", n.toFixed(2, step("nf2"), step(42), step(true)));
console.log("calls:", calls);
console.log("n.toExponential trail:", n.toExponential(2, step("ne1")));
console.log("n.toExponential multi:", n.toExponential(2, step("ne2"), step("ne3")));
console.log("calls:", calls);
console.log("n.toPrecision trail:", n.toPrecision(3, step("np1")));
console.log("n.toPrecision multi:", n.toPrecision(3, step("np2"), step(7)));
console.log("calls:", calls);
console.log("n.toLocaleString trail:", n.toLocaleString(step("en-US")));
console.log("calls:", calls);

const nf: number = 3.14;
console.log("nf.toString radix trail:", nf.toString(10, step("nft1")));
console.log("nf.toString radix multi:", nf.toString(10, step("nft2"), step("nft3")));
console.log("calls:", calls);

const bn: bigint = 255n;
console.log("bn.toString bare:", bn.toString());
console.log("bn.toString radix trail:", bn.toString(10, step("bnt1")));
console.log("bn.toString radix multi:", bn.toString(16, step("bnt2"), step("bnt3")));
console.log("calls:", calls);
console.log("bn.toString undef trail:", bn.toString(undefined, step("bnt4")));
console.log("calls final:", calls);
