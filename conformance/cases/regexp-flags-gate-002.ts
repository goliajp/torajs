// The step 2.b `flags` read of replaceAll / matchAll is a real Get: an own
// `flags` getter runs there once, and again inside @@replace / @@matchAll.
let reads = 0;
const re: any = /a/g;
Object.defineProperty(re, "flags", {
  get() {
    reads++;
    return "g";
  },
});
const s: any = "aXa";
console.log(s.replaceAll(re, "b"), reads);
console.log([...s.matchAll(re)].length, reads);
console.log("aXa".replaceAll(re, "c"), reads);
