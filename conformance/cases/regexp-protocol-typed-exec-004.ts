// An own `exec` defined through a descriptor map, and through
// defineProperty under a key held in a binding: the typed String
// methods given that RegExp run the protocol algorithms, which call it.
const r = /a/;
Object.defineProperties(r, { exec: { value: () => null } });
console.log("xax".replace(r, "b"));
console.log("xax".search(r));
console.log("xax".match(r));
console.log("xax".split(r));
const k = "exec";
const t = /x/g;
Object.defineProperty(t, k, { value: (s: string) => null });
console.log("xax".replace(t, "y"));
console.log("xax".search(t));
