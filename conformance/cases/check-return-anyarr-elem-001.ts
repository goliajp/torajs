// `return a[i]` off a local Arr<Any> (chunk 674): the elem box
// retains its payload at the return root so the array keeps its
// scope drop (the old moved-mark stranded one array per call), and
// a borrowed receiver (param) keeps the caller's reference intact.
function h(x: number): any {
  const a: any[] = [x, "some heap string that is long enough"];
  return a[0];
}
console.log(h(7));
function grab(x: number): any {
  const a: any[] = [x, "some heap string that is long enough"];
  return a[1];
}
console.log(grab(1));
function pick(a: any[], i: number): any { return a[i]; }
const xs: any[] = [1, "a genuinely long heap string beyond shortstr", true];
console.log(pick(xs, 1));
console.log(xs[1]);
console.log(pick(xs, 0));
console.log(pick(xs, 2));
