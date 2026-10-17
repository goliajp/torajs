// An own `exec` written under a key computed at run time: no spelling
// names it, so a typed String method given that RegExp runs the
// protocol algorithm (§22.2.6.11 and siblings), which calls the own
// exec — string, callback and `any` replacements alike.
function key(): string {
  return ["ex", "ec"].join("");
}
const r = /a/g;
(r as any)[key()] = (s: string) => null;
console.log("xaxa".replace(r, "b"));
console.log("xaxa".replaceAll(r, "b"));
console.log("xaxa".replace(r, (m: string) => m + "!"));
const cb = (m: string) => m + "!";
console.log("xaxa".replaceAll(r, cb));
const rv: any = "c";
console.log("xaxa".replace(r, rv));
console.log("xaxa".search(r));
console.log("xaxa".match(r));
console.log("xaxa".split(r));
console.log([..."xaxa".matchAll(r)].length);
const plain = /a+/g;
console.log("xaxa".replace(plain, "b"));
console.log("xaxa".replace(plain, (m: string) => m + "!"));
console.log("xaxa".split(plain));
