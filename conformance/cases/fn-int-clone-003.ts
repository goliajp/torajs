// recursions the integer clone must leave alone or route around:
// one reading `arguments`, a binding that is reassigned, two
// functions calling each other

function argc(n: number): number {
  if (n < 1) return arguments.length
  return argc(n - 1) + arguments.length + n
}
console.log(argc(parseInt('4', 10)), argc(Number('2.5')))

let r = function (n: number): number {
  return n < 1 ? 0 : 1 + r(n - 1)
}
const first = r
console.log(r(parseInt('5', 10)))
r = function (n: number): number {
  return n * 100
}
console.log(first(parseInt('5', 10)), r(parseInt('5', 10)))

function isEven(n: number): boolean {
  return n <= 0 ? n === 0 : isOdd(n - 1)
}
function isOdd(n: number): boolean {
  return n <= 0 ? false : isEven(n - 1)
}
console.log(isEven(parseInt('10', 10)), isOdd(Number('7')), isEven(Number('2.5')), isOdd(Number('-0')))

// the recursion throws from the bottom and every frame unwinds
class Stop {
  depth: number
  constructor(depth: number) {
    this.depth = depth
  }
}
let unwound = 0
function dive(d: number, v: number): number {
  if (d === 0) {
    if (v % 3 === 0) throw new Stop(v)
    return v
  }
  try {
    return dive(d - 1, v + 1) + 1
  } finally {
    unwound = unwound + 1
  }
}
let sum = 0
for (let i = 0; i < parseInt('6', 10); i = i + 1) {
  try {
    sum = sum + dive(parseInt('4', 10), i)
  } catch (e) {
    sum = sum + (e as Stop).depth * 1000
  }
}
console.log(sum, unwound)
