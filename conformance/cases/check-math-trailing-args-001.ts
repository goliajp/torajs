// S243 — Math.* namespace methods trailing-arg ignore per ES
// §21.3.2.*: each Math.* algorithm reads only the declared
// positional args; tora's Type::Function arity gate truncates
// trailing args + type_of's them for side effects before they
// reach the (f64, f64) / (f64,) helper ABI. Same shape as S238
// localeCompare / S239–S242 narrow trailing-arg carve-outs, but
// applied at the check arity gate for the Math.* receiver only
// (SSA-emit already handles trailing-arg trimming for Math).

// 2-useful + trailing
console.log(Math.pow(2, 3, 99));
console.log(Math.pow(2, 3, "junk" as any));
console.log(Math.atan2(1, 1, 99));
console.log(Math.atan2(1, 1, "junk" as any));
console.log(Math.imul(0x10, 0x20, 99));

// 1-useful + trailing
console.log(Math.sqrt(4, 99));
console.log(Math.sqrt(9, "junk" as any));
console.log(Math.log(8, 99));
console.log(Math.log(8, "junk" as any));
console.log(Math.log2(1024, 99));
console.log(Math.log10(100, 99));
console.log(Math.sin(0, 99));
console.log(Math.cos(0, 99));
console.log(Math.abs(-7, 99));
console.log(Math.floor(3.7, 99));
console.log(Math.ceil(3.2, 99));
console.log(Math.round(3.5, 99));
console.log(Math.trunc(3.9, 99));
console.log(Math.sign(-42, 99));
console.log(Math.cbrt(27, 99));
console.log(Math.expm1(0, 99));
console.log(Math.log1p(0, 99));
console.log(Math.clz32(1, 99));
console.log(Math.fround(1.5, 99));
