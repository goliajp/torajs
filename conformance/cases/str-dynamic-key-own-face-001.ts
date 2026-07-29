// §10.4.3 String exotic [[GetOwnProperty]] through a DYNAMIC key on
// an `any`-erased string receiver. The member probe pair had no Str
// arm at all: `s[k]` with k spelling "length" or a canonical index
// fell to the builtin-method reify probe, which only knows method
// names, so both answered a silent `undefined`. The literal form
// (`s["length"]` pre-lowers to the static length read) and the number
// form (`s[1]` rides the index lane) were fine, which is why it
// stayed hidden — test262's propertyHelper reads every key
// dynamically.
const s: any = "abcd";
const L = "length";
const I = "2";
console.log(s[L]);
console.log(s[I]);

// literal + number forms keep working
console.log(s["length"]);
console.log(s[1]);

// non-own keys stay absent: past the end, non-canonical spellings,
// negative and fractional indices all read through to nothing
const OOB = "9";
const LEADZERO = "01";
const FRAC = "1.5";
const NEG = "-1";
console.log(s[OOB], s[LEADZERO], s[FRAC], s[NEG]);

// the method surface still reifies past the own face
const M = "toUpperCase";
console.log(typeof s[M]);
console.log(s[M].call(s));

// the answer is a real string value, not a look-alike
const c = s[I];
console.log(c === "c", c + "!", c.length, typeof c);

// a heap (non-ShortStr) receiver takes the cell arm
const long: any = "abcdefghijklmnop";
const J = "10";
console.log(long[J], long[L]);

// non-ASCII: a Latin-1 code unit, and a wide one
const acc: any = "aéc";
console.log(acc["1"], acc[L]);
const cjk: any = "中文x";
console.log(cjk["0"], cjk["1"], cjk[L]);

// an astral character indexes to its surrogate halves
const astral: any = "a\u{1F600}b";
console.log(astral[L]);
console.log(astral["1"].charCodeAt(0), astral["2"].charCodeAt(0), astral["3"]);

// a String wrapper's inherent index face answers ahead of its expando
const w: any = new String("abcd");
console.log(w[L], w[I], w[OOB]);

// a key built at runtime (owned temp — the probe borrows it)
const n = 3;
console.log(s["" + n]);

// repeated reads answer equal values (the face interns its cells)
console.log(s[I] === s[I], long["10"] === long["10"]);
