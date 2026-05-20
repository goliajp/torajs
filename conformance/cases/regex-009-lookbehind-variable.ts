// Phase 1c.4.b: variable-width lookbehind — body that can match
// multiple lengths (alternation, quantifier). Drives the sub_probe
// candidate-start loop which scans j ∈ [0..pos] until any candidate
// yields a length-exact match.

// Alternation in body — body matches either of two lengths
console.log(/(?<=ab|abc)d/.exec("abcd")[0]);
console.log(/(?<=ab|abc)d/.test("abcd"));
console.log(/(?<=ab|abc)d/.test("acd"));

// Quantifier in body — `a+` matches 1..n preceding 'a's
console.log(/(?<=a+)b/.exec("aaab")[0]);
console.log(/(?<=a+)b/.test("aaab"));
console.log(/(?<=a+)b/.test("xb"));

// Mixed-length alternation
console.log(/(?<=cat|tiger)claw/.exec("catclaw")[0]);
console.log(/(?<=cat|tiger)claw/.exec("tigerclaw")[0]);
console.log(/(?<=cat|tiger)claw/.test("dogclaw"));

// Negative lookbehind with variable-width body
console.log(/(?<!a+)b/.test("aaab"));
console.log(/(?<!a+)b/.exec("xb")[0]);
console.log(/(?<!a+)b/.test("xb"));

// Lookbehind body with char class + quantifier
console.log(/(?<=\d+)px/.exec("100px")[0]);
console.log(/(?<=\d+)px/.test("100px"));
console.log(/(?<=\d+)px/.test("abcpx"));
