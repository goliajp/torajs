// §13.15.1 yield-target early-error legal faces: yield as a VALUE or
// inside an INDEX of a valid target must keep working after the
// target-position rejects landed ((yield) = v / (yield)++ / ++(yield)
// are now parse-time SyntaxErrors).
function* g1(): any {
  let x: any = 0;
  x = yield 1;
  return x;
}
const it1: any = g1();
console.log(it1.next().value);
console.log(it1.next(42).value);

function* g2(arr: any): any {
  arr[yield 0] = 7;
}
const a: any = [1, 2];
const it2: any = g2(a);
it2.next();
it2.next(1);
console.log(a[1]);

function* g3(): any {
  let n: any = 0;
  n += yield 5;
  return n;
}
const it3: any = g3();
console.log(it3.next().value);
console.log(it3.next(3).value);
