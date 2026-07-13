// toLocaleLowerCase / toLocaleUpperCase locale-tailored casing (tr/az/lt)
// Turkic lower: U+0130 -> i
console.log("\u0130".toLocaleLowerCase("tr"));
// Turkic lower: I + dot-above -> i (After_I eats the dot)
console.log("I\u0307".toLocaleLowerCase("tr"));
// Turkic lower: ccc-220 mark intervenes, dot still eaten
console.log("I\u0323\u0307".toLocaleLowerCase("tr"));
// Turkic lower: base letter intervenes, dot survives, I -> dotless
console.log("IA\u0307".toLocaleLowerCase("tr"));
// Turkic lower: ccc-230 intervenes, dot survives, I -> dotless
console.log("I\u0300\u0307".toLocaleLowerCase("tr"));
// Turkic lower: supplementary ccc-220 intervenes, dot eaten
console.log("I\uD800\uDDFD\u0307".toLocaleLowerCase("tr"));
// Turkic lower: bare I -> dotless i (U+0131)
console.log("I".toLocaleLowerCase("tr"));
console.log("I".toLocaleLowerCase("az"));
// Turkic upper: i -> U+0130
console.log("i".toLocaleUpperCase("tr"));
console.log("i".toLocaleUpperCase("az"));
// dotless i uppercases to plain I in every locale (simple table)
console.log("\u0131".toLocaleUpperCase("tr"));
// Lithuanian lower: I/J/Ogonek-I gain dot-above under More_Above
console.log("I\u0300".toLocaleLowerCase("lt"));
console.log("J\u0300".toLocaleLowerCase("lt"));
console.log("\u012E\u0300".toLocaleLowerCase("lt"));
// Lithuanian lower: More_Above across an intervening ccc-220 mark
console.log("I\u0325\u0300".toLocaleLowerCase("lt"));
// Lithuanian lower: no following above-accent -> plain fold
console.log("Iw".toLocaleLowerCase("lt"));
// Lithuanian lower: accented capital I expands unconditionally
console.log("\u00CC".toLocaleLowerCase("lt"));
console.log("\u00CD".toLocaleLowerCase("lt"));
console.log("\u0128".toLocaleLowerCase("lt"));
// Lithuanian upper: dot-above after soft-dotted base is deleted
console.log("i\u0307".toLocaleUpperCase("lt"));
console.log("j\u0323\u0307w".toLocaleUpperCase("lt"));
// Lithuanian upper: capital I is not soft-dotted, dot survives
console.log("I\u0307".toLocaleUpperCase("lt"));
// supplementary-plane soft-dotted base (math italic small i)
console.log("\uD835\uDC56\u0307".toLocaleUpperCase("lt"));
// non-tailored locales take the default fold
console.log("\u0130".toLocaleLowerCase("und"));
console.log("\u0130".toLocaleLowerCase());
console.log("I".toLocaleLowerCase("en-US"));
// primary-subtag match is case-insensitive and region-tolerant
console.log("I".toLocaleLowerCase("TR-TR"));
// Final_Sigma still applies under a tailored locale
console.log("A.\u03A3".toLocaleLowerCase("tr"));
// any-tier receiver
const anyLow: any = "\u0130";
console.log(anyLow.toLocaleLowerCase("tr"));
const anyUp: any = "i";
console.log(anyUp.toLocaleUpperCase("az"));
// Substr (string view) receiver
const big = "xx\u0130yy";
console.log(big.slice(2, 3).toLocaleLowerCase("tr"));
