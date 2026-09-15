// §28.1.10 — Reflect.preventExtensions reports a Proxy's refusal as
// false; Object.preventExtensions turns the same refusal into a
// TypeError.
const refusing = new Proxy({}, { preventExtensions() { return 0 as any; } });
console.log("reflect refusal", Reflect.preventExtensions(refusing));
try {
  Object.preventExtensions(refusing);
  console.log("object no throw");
} catch (e: any) {
  console.log("object", e instanceof TypeError ? "TypeError" : "other");
}
const accepting = new Proxy({}, { preventExtensions(t: any) { Object.preventExtensions(t); return true; } });
console.log("reflect accept", Reflect.preventExtensions(accepting), Object.isExtensible(accepting));
const lying = new Proxy({}, { preventExtensions() { return true; } });
try {
  Reflect.preventExtensions(lying);
  console.log("lying no throw");
} catch (e: any) {
  console.log("lying", e instanceof TypeError ? "TypeError" : "other");
}
const plain: any = {};
console.log("plain", Reflect.preventExtensions(plain), Object.isExtensible(plain));
const rp: any = Reflect.preventExtensions;
console.log("detached", rp(refusing), rp({}));
