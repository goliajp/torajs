// V3-18 m1.h.25 — parseInt without an explicit radix uses
// auto-detect (10 by default, 16 if "0x" / "0X" prefix). Per
// JS spec §19.2.5 / §21.1.3.7. Pre-fix tora hardcoded radix=10
// at the call site, so parseInt("0x10") returned 0 instead of 16.
//
// Runtime helper already had the auto-detect path; the fix is
// simply passing 0 (sentinel) to the runtime instead of 10
// when the user didn't supply a radix.

console.log(parseInt("10"))           // 10
console.log(parseInt("10", 2))         // 2
console.log(parseInt("ff", 16))        // 255
console.log(parseInt("0x10"))          // 16 — auto-detect hex
console.log(parseInt("0X10"))          // 16 — uppercase X
console.log(parseInt("0xabc"))         // 2748
console.log(parseInt(""))              // NaN
console.log(parseInt("abc"))           // NaN
console.log(parseInt("100", 8))        // 64
console.log(parseInt("-0x10"))         // -16 — sign + auto-radix
console.log(parseInt("  17  "))        // 17 — whitespace trim
