// Two functions each bind a local arrow under the same name, and only one
// of them takes a function-typed parameter: the other still receives the
// very function value passed to it, with every argument it is called with.
function g(): number { return arguments.length }
function h(x?: number): any { return x === undefined ? "u" : x * 2 }
let seen: any = null

function a(k: number): number {
  const inner = (f: () => number) => f()
  return inner(() => k) + inner(g)
}
function b(): any {
  const inner = (y: any) => { seen = y; return y(1, 2, 3) + ":" + (y === g) + ":" + y.name }
  return inner(g)
}
function c(): any {
  const inner = (y: (x: number) => any) => y(4)
  return inner(h)
}
function d(k: number): any {
  const inner = (f: () => any) => f()
  return inner(() => k) + inner(h)
}
console.log(b(), seen === g)
console.log(c())
console.log(a(3))
console.log(d(1))
