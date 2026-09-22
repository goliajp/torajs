// §22.2.6.11 step 14.f — a capture group that did not participate is
// undefined, so a replace callback's capture parameter is
// `string | undefined`. Seeded as `string`, `typeof` folded to
// "string", `=== undefined` folded to false and a method call read the
// sentinel's own text.
console.log("a".replace(/(b)?a/, (m, p1) => typeof p1));
console.log("ba".replace(/(b)?a/, (m, p1) => typeof p1));
console.log("a".replace(/(b)?a/, (m, p1) => (p1 === undefined ? "U" : p1)));
console.log("a".replace(/(b)?a/, (m, p1) => p1 || "-"));
console.log("a".replace(/(b)?a/, (m, p1) => p1));
console.log("a".replace(/(b)?a/, (m, p1) => `<${p1}>`));
console.log("xay".replace(/(b)?a/, (m, p1, off, input) => `${typeof p1}|${off}|${input}`));
console.log("a-a".replaceAll(/(b)?a/g, (m, p1) => typeof p1));
console.log("a".replace(/(b)?(c)?a/, (m, p1, p2) => `${p1 + p2}`));
console.log("a".replace(/(b)?a/, (m, p1) => `${p1 + 1}`));
console.log("ba".replace(/(b)?a/, (m, p1) => `${p1 + 1}`));
console.log("a".replace(/(b)?a/, (m, p1) => (p1 === undefined ? "gone" : p1.toUpperCase())));
