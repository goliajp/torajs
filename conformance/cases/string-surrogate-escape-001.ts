// §12.9.4.2 SV — paired \uD8xx\uDCxx escapes form ONE supplementary
// code point. Pre-fix each half independently became U+FFFD, so every
// astral literal spelled with surrogate escapes was destroyed.
let g = "\uD835\uDCA2"; // MATHEMATICAL SCRIPT CAPITAL G U+1D4A2
console.log(g.length);
console.log(g.charCodeAt(0).toString(16));
console.log(g.charCodeAt(1).toString(16));
console.log(g === "\u{1D4A2}");

// escape form and literal form must be identical
console.log("\uD835\uDC22" === "𝐢");

// Final_Sigma context through a supplementary predecessor
console.log(("\uD835\uDCA2\u03A3").toLowerCase() === "𝒢ς");

// paired escape mid-string
let mix = "a\uD835\uDCA2b";
console.log(mix.length);
console.log(mix.codePointAt(1).toString(16));

// high surrogate NOT followed by a low-surrogate escape stays lone
// (internal WTF-8 residual: lone halves read back as U+FFFD)
let lone = "\uD835x";
console.log(lone.length);

// non-surrogate escapes keep their one-to-one decoding
console.log("Aé中" === "Aé中");
