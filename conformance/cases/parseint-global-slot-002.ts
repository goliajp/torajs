// a user binding of the parser's name answers before the builtin, so
// the binding takes whatever the user function returns
function parseInt(s: string, r: number): string {
  return s + "/" + r;
}
const n = parseInt("3", 10);
function show(): string {
  return "n=" + n;
}
console.log(show(), typeof n);
