// P3.1-e.1 — torajs-str::transform::case bun-parity fixture.
// ASCII-only fold contract (per stdlib.md): byte values 'a'..'z'
// flip with 'A'..'Z'; every other byte (digits / punct / multi-byte
// UTF-8 leading & continuation bytes) passes through. Inputs here
// stay ASCII-only so the byte-level fold matches bun's Unicode
// fold exactly.

const upper_basic: string = "hello, world!".toUpperCase();
const lower_basic: string = "HELLO, WORLD!".toLowerCase();

console.log(upper_basic);
console.log(lower_basic);

// Mixed case input.
console.log("AbCdEfG".toUpperCase());
console.log("AbCdEfG".toLowerCase());

// Digits + punctuation pass through.
console.log("ABC123def!@#".toUpperCase());
console.log("ABC123def!@#".toLowerCase());

// Empty string.
console.log("[" + "".toUpperCase() + "]");
console.log("[" + "".toLowerCase() + "]");

// Single-char ASCII.
console.log("a".toUpperCase());
console.log("Z".toLowerCase());

// Round-trip: upper(lower(x)) === upper(x) for ASCII letters.
console.log("HELLO".toLowerCase().toUpperCase());

// Length preserved.
const long_in: string = "Quick Brown FOX jumps over THE lazy dog";
const long_up: string = long_in.toUpperCase();
const long_lo: string = long_in.toLowerCase();
console.log(long_up);
console.log(long_lo);
console.log(long_up.length === long_in.length ? "len-up-eq" : "len-up-diff");
console.log(long_lo.length === long_in.length ? "len-lo-eq" : "len-lo-diff");
