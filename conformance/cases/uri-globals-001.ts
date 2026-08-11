// §19.2.6 URI handling globals — encode/decode kernels + URIError.
console.log(encodeURI("http://a.b/c?d=e&f#g"));
console.log(encodeURIComponent("http://a.b/c?d=e&f#g"));
console.log(encodeURI("a b~c*d'e(f)g!h"));
console.log(encodeURIComponent(";,/?:@&=+$"));
console.log(encodeURI("é世😀"));
console.log(encodeURIComponent("é世😀"));
console.log(decodeURI("%C3%A9%E4%B8%96%F0%9F%98%80"));
console.log(decodeURIComponent("%C3%A9%E4%B8%96%F0%9F%98%80"));
// decodeURI preserves reserved escapes with ORIGINAL case
console.log(decodeURI("a%2fb%3Bc%20d"));
console.log(decodeURIComponent("a%2fb%3Bc%20d"));
// roundtrip
console.log(decodeURIComponent(encodeURIComponent("q=a b&r=é/世#x")));
// malformed → URIError
for (const bad of ["%", "%A", "%G1", "%C3", "%C3%C3", "%C0%80", "%ED%A0%80", "%80"]) {
  try {
    decodeURIComponent(bad);
    console.log("no-throw", bad);
  } catch (e) {
    console.log(e instanceof URIError, e instanceof Error, e.name);
  }
}
// encode: lone surrogate raises
try {
  encodeURIComponent(String.fromCharCode(0xd800));
  console.log("no-throw-lone");
} catch (e) {
  console.log(e instanceof URIError, e.name);
}
// ToString coercion + omitted arg
console.log(encodeURI(), decodeURI());
const o: any = { toString() { return "a b"; } };
console.log(encodeURIComponent(o));
