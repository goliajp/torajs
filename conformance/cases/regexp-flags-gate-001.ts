// String.prototype.replaceAll / matchAll step 2.b reads the pattern's
// `flags` (§22.1.3.13 / .20): a RegExp whose own `flags` lacks "g" is a
// TypeError even when its matcher is global, on a typed and an any-typed
// receiver alike.
const re1: any = /a/;
try {
  console.log("aXa".replaceAll(re1, "b"));
} catch (e) {
  console.log("no g", e instanceof TypeError);
}
const re2: any = /a/g;
Object.defineProperty(re2, "flags", { value: "" });
const s: any = "aXa";
try {
  console.log(s.replaceAll(re2, "b"));
} catch (e) {
  console.log("any replaceAll", e instanceof TypeError);
}
try {
  console.log([...s.matchAll(re2)].length);
} catch (e) {
  console.log("any matchAll", e instanceof TypeError);
}
try {
  console.log("aXa".replaceAll(re2, "b"));
} catch (e) {
  console.log("typed replaceAll", e instanceof TypeError);
}
try {
  console.log([..."aXa".matchAll(re2)].length);
} catch (e) {
  console.log("typed matchAll", e instanceof TypeError);
}
console.log("aXa".replace(re2, "b"));
