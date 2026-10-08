// a self-recursive number function runs on integers when its
// argument is one, and exactly as before when it is not

function f(n: number): number {
  if (!(n >= 2 && n <= 30)) return n
  return f(n - 1) + f(n - 2)
}

console.log(f(parseInt('10', 10)))
console.log(f(Number('2.5')))
console.log(1 / f(Number('-0')))
console.log(f(Number('x')))
console.log(f(Number('9007199254740992')), f(Number('9223372036854775808')))
console.log(f(Number('Infinity')), f(Number('-Infinity')), f(Number('-7')))
console.log(f(parseInt('0', 10)), f(parseInt('1', 10)), f(parseInt('30', 10)))

// the same function from an integer and from a fraction
const whole = Number('7')
const part = Number('7.5')
console.log(f(whole), f(part))

// through a function value: the original
const fs: Array<(n: number) => number> = [f, f]
console.log(fs[1](Number('12')), fs[0](Number('6.5')))
function apply(g: (n: number) => number, x: number): number {
  return g(x)
}
console.log(apply(f, Number('11')), apply(f, Number('3.25')))
