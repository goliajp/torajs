// A function expression with a defaulted parameter that reads
// `arguments`, called with different counts, sees each call's own.
let args: any
const f = function (x: any = 1) {
  args = arguments
  return x
}
console.log(f(), args.length)
console.log(f(undefined, 5), args.length, args[1])
console.log(f(3), args.length, args[0])

const g = function (a: number, b: any = 'b', c: any = (args = arguments)) {
  return a + ':' + b + ':' + arguments.length
}
console.log(g(1))
console.log(g(1, 2, 3, 4))
console.log(args.length)
