// the rpn evaluator's stack shape: a fixed literal used as a stack
// through an index, pushed past its literal length on a long input
function evalRpn(expr: string): number {
  const stack: number[] = [0, 0, 0, 0]
  let sp = 0
  const parts = expr.split(' ')
  for (let i = 0; i < parts.length; i = i + 1) {
    const c0 = parts[i].charCodeAt(0)
    if (c0 >= 48 && c0 <= 57) {
      stack[sp] = c0 - 48
      sp = sp + 1
    } else {
      const b = stack[sp - 1]
      const a = stack[sp - 2]
      sp = sp - 2
      stack[sp] = c0 === 43 ? a + b : c0 === 45 ? a - b : a * b
      sp = sp + 1
    }
  }
  console.log(stack.length)
  return stack[0]
}
console.log(evalRpn('3 4 + 2 * 5 +'))
console.log(evalRpn('1 2 3 4 5 6 7 8 9 + + + + + + + +'))
console.log(evalRpn('9 8 - 7 6 - * 5 4 3 2 1 + + + + *'))
