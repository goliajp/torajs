// A type argument written as the caller's own type parameter does not
// pin the callee's: inside `outer<U>`, `each<U>(..)` says nothing the
// arrow (lifted out of `outer`) could name, so its parameter is not
// typed `U`. A type argument with no type parameter in it still pins.
function each<T>(f: (t: T) => number, v: T): number {
  return f(v);
}
function hof<T>(f: (g: (x: number) => number) => T): T {
  return f((x) => x * 2);
}
function outer<U>(u: U): number {
  return each<U>((t) => 1, u);
}
function outerArr<U>(u: U): number {
  return each<U[]>((t) => 2, [u, u]);
}
console.log(outer(5), outer("s"), outerArr(true));
console.log(each<number>((t) => t + 1, 41));
console.log(hof<number>((g) => g(6)));
console.log(hof<string>((g) => "v" + g(7)));
