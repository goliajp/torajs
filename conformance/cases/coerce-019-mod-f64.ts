// V3-18 m1.h.41 — Number `%` with f64 operand(s) routes to LLVM
// frem (IEEE fmod-shaped). Per JS spec §13.10 numeric remainder
// is fmod, not srem; pre-fix tora's lower_binop hard-rejected
// f64 % anything with "mod op requires i64 operands".

console.log(7.5 % 2)            // 1.5
console.log(7.5 % 2.5)          // 0
console.log(7 % 2.5)            // 2
console.log(0.1 % 0.03)         // 0.010000000000000009 (fmod)
console.log(-7.5 % 2)           // -1.5
console.log(7.5 % -2)           // 1.5
console.log(Infinity % 1)       // NaN
console.log(1 % Infinity)       // 1

// Pure i64 path still works (no regression).
console.log(7 % 3)              // 1
console.log(-7 % 3)             // -1
