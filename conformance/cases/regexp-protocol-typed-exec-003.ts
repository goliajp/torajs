// §22.1.3.19 / .13 / .20 / .22 step 2 calls the pattern's builtin
// @@replace / @@match / @@search / @@split, whose §22.2.7.1 RegExpExec
// calls an own `exec` — here one installed through Reflect.set.
const r = /a/;
Reflect.set(r, "exec", () => null);
console.log("xax".replace(r, "b"));
console.log("xax".search(r));
console.log("xax".match(r));
console.log("xax".split(r));
const g = /a/g;
console.log(Reflect.set(g, "exec", (s: string) => null));
console.log("xaxa".replaceAll(g, "b"));
