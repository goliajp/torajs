// ES §23.1.3.1 Array.prototype.at — non-integer index routes
// through ToIntegerOrInfinity (step 2). Fractional values trunc
// toward 0; NaN → 0. Pre-fix tr panicked with "GPR materialization
// can't hold an f64" because the inline negative-wrap chain
// (ICmp/Add/LoadDyn) was emitted directly on the f64 operand.

const a = [10, 20, 30, 40, 50];

console.log(a.at(1.5));   // ToIntegerOrInfinity(1.5)  =  1  -> 20
console.log(a.at(1.9));   //                            =  1  -> 20
console.log(a.at(-1.5));  // ToIntegerOrInfinity(-1.5) = -1  -> 50
console.log(a.at(0.5));   //                            =  0  -> 10
console.log(a.at(-0.5));  // ToIntegerOrInfinity(-0.5) =  0  -> 10
console.log(a.at(2.7));   //                            =  2  -> 30
console.log(a.at(NaN));   // ToIntegerOrInfinity(NaN)  =  0  -> 10
console.log(a.at(0.0));   //                            =  0  -> 10
console.log(a.at(-0.0));  //                            =  0  -> 10

// typed Array<string> — same path.
const s: string[] = ["a", "b", "c", "d"];
console.log(s.at(1.5));   // "b"
console.log(s.at(-1.5));  // "d"
console.log(s.at(NaN));   // "a"
