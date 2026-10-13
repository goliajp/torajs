// A function with no return annotation converts nothing, so a value of
// any kind it hands back reaches the function declared to return a
// scalar that returns its call, through any number of such functions
// in between.
function inner() {
  return "s" as any;
}
function outer(): number {
  return inner();
}
function obj(n: number) {
  if (n > 0) return { a: n } as any;
  return n;
}
function objOuter(n: number): number {
  return obj(n);
}
function flag() {
  const v: any = 1;
  return v;
}
function relay() {
  return flag();
}
function asString(): string {
  return relay();
}
function same() {
  const v: any = 2;
  return v;
}
function sameOuter(): number {
  return same() + 1;
}
console.log(outer(), objOuter(1), objOuter(-1));
console.log(asString(), typeof asString(), sameOuter());
