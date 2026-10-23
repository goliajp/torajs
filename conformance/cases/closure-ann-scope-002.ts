// a parameter in one function and a same-named for-of binding or catch
// binding in another: each site's arrows are typed from the binding its
// own scopes declare
function nums(fs: Array<(n: number) => number>): number {
  fs.push((n) => n + 1);
  return fs[0](41);
}
function loopStrs(xs: Array<Array<(s: string) => string>>): string {
  let out = "";
  for (const fs of xs) {
    fs.push((s) => s + "!");
    out += fs[fs.length - 1]("a");
  }
  return out;
}
function caught(): string {
  try {
    throw [];
  } catch (fs: any) {
    fs.push((t: any) => t + "?");
    return fs[0]("b");
  }
}
console.log(nums([]));
const xss: Array<Array<(s: string) => string>> = [[], []];
console.log(loopStrs(xss));
console.log(caught());
