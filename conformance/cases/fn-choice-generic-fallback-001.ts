// a choice between an `any` value and a declared function
// (`options?.decode || decode`, the cookie package's shape) compiles
// whether or not the function's parameters are annotated, and calls the
// chosen one when the function's are
function decodeTyped(str: string): string {
  return str.indexOf("%") === -1 ? str : decodeURIComponent(str);
}
function parse(s: string, options?: any): string {
  const dec = options?.decode || decodeTyped;
  return dec(s);
}
console.log(parse("a%20b"), parse("x", { decode: (v: string) => v + "!" }));
function decode(str) {
  return str.indexOf("%") === -1 ? str : decodeURIComponent(str);
}
export function parseUncalled(s, options) {
  const dec = options?.decode || decode;
  return dec(s);
}
