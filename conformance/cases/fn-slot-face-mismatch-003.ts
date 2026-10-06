// a closure calling a slot param of its enclosing function reads the
// stored function's answer the same way a direct call does
function apply2(f: (s: any) => number, v: any): any {
  const g = () => f(v);
  return g();
}
console.log(apply2((s: string) => s.length, 5), apply2((s: string) => s.length, "abc"));
function nested(f: (s: any) => number) {
  const outer = (x: any) => {
    const inner = () => f(x);
    return inner();
  };
  return [outer("xy"), outer(7)];
}
console.log(nested((s: string) => s.length).join(" "));
