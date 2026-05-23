// P3.1-f bun-parity fixture — single-block split + per-char + edge cases.
// All inputs ASCII so byte-level Substr matches bun's UTF-16 split.

console.log("[" + "a,b,c".split(",").join("|") + "]");          // a|b|c
console.log("[" + "abc".split("").join("|") + "]");              // a|b|c
console.log("[" + "abc".split("z").join("|") + "]");             // abc
console.log("[" + "foo<>bar<>baz".split("<>").join("|") + "]");  // foo|bar|baz
console.log("[" + "a,b,".split(",").join("|") + "]");            // a|b|  (trailing empty)
console.log("[" + ",a,b".split(",").join("|") + "]");            // |a|b   (leading empty)
console.log("[" + "".split(",").join("|") + "]");                // (empty s → [""])
console.log("[" + "aaaa".split("aa").join("|") + "]");           // ||     (non-overlap)
console.log("[" + "x".split("x").join("|") + "]");               // | (single match)
console.log("[" + "one,,two".split(",").join("|") + "]");        // one||two (empty middle)

// .length after split — exercises array len read from split block.
console.log("len:" + "a,b,c,d".split(",").length);  // 4
console.log("len:" + "abc".split("").length);        // 3
console.log("len:" + "no-sep".split(",").length);    // 1
