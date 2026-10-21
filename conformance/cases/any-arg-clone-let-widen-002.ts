// The copies the compiler makes of a function body (for a call with an
// `any` argument, for a generic call, for the construct face of a
// function expression that reads `new.target`) each hold a binding
// assigned an `any` value as it is, and a module binding such a copy
// writes still reads right everywhere else.

let total = 0
function add(t: string): number {
  total = total + t.length
  return total
}
const word: any = 'abc'
console.log(add(word), add('xy'))
console.log('hello'.charCodeAt(total - 1), total.toFixed(1), typeof total)
function later(): number {
  return 'hello'.charCodeAt(total - 4)
}
console.log(later())

function gen<T>(t: T, s: string): number {
  let i = 0
  i = i + (t as any)
  return s.charCodeAt(i)
}
console.log(gen(1, 'hello'), gen('q', 'hello'))

const one: any = 1
const F = function (this: any) {
  let i = 0
  i = i + one
  console.log(new.target === undefined, 'hello'.charCodeAt(i), i)
}
F()
new (F as any)()
