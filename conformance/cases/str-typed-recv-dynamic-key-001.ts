// §10.4.3 through a STATICALLY string-typed receiver. `s[k]` with a
// string / symbol / any key used to reject the whole program ("index
// must be number, got String") — only a number key was admitted. The
// three outcomes (the character, `length`, a method or a miss) have
// no common narrower type, so the key domain widens the answer to any
// exactly the way an array receiver's does.
const s: string = "abcd";

// string key
const L = "length";
const I = "2";
console.log(s[L]);
console.log(s[I]);

// a number key keeps the narrow character lane
console.log(s[1]);
const n = 3;
console.log(s[n]);

// any key — the runtime tag picks the lane
const k1: any = "1";
const k2: any = 2;
console.log(s[k1], s[k2]);

// symbol key misses (no own symbol face on a string)
const sym: symbol = Symbol("x");
console.log(s[sym]);

// misses and non-canonical spellings. A LITERAL key never reaches
// the index rule — it pre-lowers to a member access — so an unknown
// NAME (`s["nope"]`) still meets the member checker's reject, exactly
// as `s.nope` does. That face is the member checker's, not this one's.
console.log(s["9"], s["01"], s["-1"], s["1.5"]);
// the dynamic spelling of that same miss does read through
const MISS = "nope";
console.log(s[MISS]);

// the method surface still reads through
console.log(typeof s["toUpperCase"]);

// a Substr receiver takes the same route
const sub: string = "xxabcdyy".slice(2, 6);
console.log(sub[L], sub[I], sub["9"]);

// a key built at runtime
const built = "" + 1;
console.log(s[built]);

// non-ASCII and wide code units
const acc: string = "aéc";
console.log(acc[L], acc["1"]);
const cjk: string = "中文x";
console.log(cjk[L], cjk["0"], cjk["1"]);

// the answer is a real string value
const c = s[I];
console.log(c === "c", c + "!", (c as string).length);
