// A function expression's default that reads `arguments` reads the
// function's own arguments object, even when the default assigns.
let seen: any
const f = function (x: any = (seen = arguments)) {
  return x
}
f()
console.log(typeof seen, seen.length)

const g = function (x: any = arguments) {
  return x.length + ':' + x[1]
}
console.log(g(undefined, 7))

function outer(a: number, b: number) {
  const inner = function (y: any = arguments.length) {
    return y
  }
  return inner() + ':' + arguments.length
}
console.log(outer(1, 2))
