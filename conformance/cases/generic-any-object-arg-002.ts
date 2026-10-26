// An any value handed to an object parameter of a function whose other
// parameter is unannotated (a type variable), called directly and
// through a typed function value, with the trailing argument given and
// left to the undefined pad (§10.2.1.4): the parameter holds the value
// as it is, an object or an array, and the padded one reads undefined.
interface P {
  x: number;
}
function f(p: P, t) {
  return p.x + (t === undefined ? 1 : 2);
}
const g: (p: P, t?: any) => number = f;
const a: any = JSON.parse('{"x":1.5}');
const b: any = [7];
console.log(f(a), f(a, 0), g(a), g(a, 0), String(f(b)), String(g(b, 1)));
