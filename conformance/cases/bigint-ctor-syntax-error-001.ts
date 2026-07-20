// RFC 20260720-ctor-static-reflection 刀 5b-1 — BigInt(<string>) is
// §21.2.1.1 ToBigInt → §7.1.14 StringToBigInt (STRICT grammar).
// Parse failure raises a catchable SyntaxError (new runtime slot 4);
// the old lenient path silently answered 0n.

// ---- strict grammar accepts ----
console.log(BigInt("300"));            // 300n
console.log(BigInt("-5"));             // -5n
console.log(BigInt("+7"));             // 7n
console.log(BigInt("  12  "));         // 12n  (edge whitespace ok)
console.log(BigInt(""));               // 0n   (empty → 0n per §7.1.14)
console.log(BigInt("0x10"));           // 16n
console.log(BigInt("0o17"));           // 15n
console.log(BigInt("0b101"));          // 5n

// ---- strict grammar rejects → SyntaxError ----
function probe(s: string): void {
  try {
    BigInt(s);
    console.log("no throw");
  } catch (e) {
    console.log((e as Error).name, "|", (e as Error).message, "|", e instanceof SyntaxError);
  }
}
probe("abc");        // SyntaxError | Failed to parse String to BigInt | true
probe("12.5");       // decimal point not in the grammar
probe("1e3");        // exponent not in the grammar
probe("-0x10");      // sign + radix prefix not in the grammar
probe("12px");       // trailing garbage
probe("1 2");        // interior whitespace
