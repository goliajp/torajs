function evalRpn(expr: string): number {
  const stack: number[] = []
  const parts = expr.split(' ')
  for (let i = 0; i < parts.length; i = i + 1) {
    const tok = parts[i]
    const c0 = tok.charCodeAt(0)
    if (c0 >= 48 && c0 <= 57) {
      stack.push(c0 - 48)
    } else {
      const b = stack[stack.length - 1]
      const a = stack[stack.length - 2]
      stack.pop()
      stack.pop()
      if (c0 === 43) stack.push(a + b)
      else if (c0 === 45) stack.push(a - b)
      else stack.push(a * b)
    }
  }
  return stack[0]
}

let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  total = total + evalRpn('3 4 + 2 * 5 +')
}
console.log(total)
