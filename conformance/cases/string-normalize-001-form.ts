// V3-18 m1.h.48 — String.normalize accepts an optional form arg
// ("NFC" / "NFD" / "NFKC" / "NFKD") per JS spec §21.1.3.13.
// Pre-fix tora declared with 0 fixed params so 1-arg calls
// failed at the arity check.
//
// tora's byte-Str ASCII-only path returns the receiver identity
// for any form (multi-byte UTF-8 normalization is deferred to
// v1.0 with the rest of Unicode work). For ASCII inputs (the
// dominant case) the result is byte-equal to bun.

console.log("hello".normalize())             // hello
console.log("hello".normalize("NFC"))        // hello
console.log("hello".normalize("NFD"))        // hello
console.log("hello".normalize("NFKC"))       // hello
console.log("hello".normalize("NFKD"))       // hello

// Roundtrip through trim + normalize.
console.log("  abc  ".trim().normalize())    // abc
