// <string>.replace/replaceAll(regex, cb) — single-param callback infers
// (string) so an untyped `(m) => …` arrow is accepted (§22.1.3.19).
console.log("abc".replace(/b/, (m) => m.toUpperCase()));
console.log("a1b2c3".replace(/[0-9]/g, (m) => `<${m}>`));
console.log("Hello World".replaceAll(/o/g, (m) => m + m));
console.log("x".replace(/x/, (m) => m.repeat(3)));
const suffix = "!";
console.log("hi".replace(/i/, (m) => m + suffix));
console.log("ab".replace(/a/, (m: string) => m.toUpperCase()));
console.log("banana".replace("a", (m) => m.toUpperCase()));
console.log("a b c".replace(/\w/g, (m) => m.toUpperCase()));
