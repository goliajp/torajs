// §19.2.6 URI globals as first-class values on the globalThis fill
// list — cells dispatch through the real Encode/Decode kernels.
const g: any = globalThis;
const dc = g["decodeURIComponent"];
console.log(typeof dc, dc.name, dc.length);
console.log(dc("%C3%A9%2F%3B"));
const du = g["decodeURI"];
console.log(du.name, du("a%2fb%20c"));
const ec = g["encodeURIComponent"];
console.log(ec.name, ec("a b/;é"));
const eu = g["encodeURI"];
console.log(eu.name, eu("a b/;é"));
// malformed through the cell raises the same URIError
try {
  dc("%C0%80");
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof URIError, e.name);
}
// descriptor face
const d = Object.getOwnPropertyDescriptor(globalThis, "decodeURI");
console.log(typeof d, d.writable, d.enumerable, d.configurable, typeof d.value);
