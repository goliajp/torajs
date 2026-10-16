// Calls through a vtable row, a fn value and a closure reach functions
// that hand a throw back only in the pending-throw flag; a direct call
// to a throwing function after a caught indirect throw returns normally.
class Shape {
  area(n: number): number {
    if (n < 0) throw new Error('shape ' + n)
    return n
  }
}
class Square extends Shape {
  area(n: number): number {
    if (n > 4) throw new Error('square ' + n)
    return n * n
  }
}
class Circle extends Shape {
  area(n: number): number {
    return checked(n) * 3
  }
}

function checked(n: number): number {
  if (n === 3) throw new Error('checked ' + n)
  return n + 1
}

function apply(f: (n: number) => number, n: number): number {
  return f(n) + 1
}

function main(): void {
  const shapes: Shape[] = [new Shape(), new Square(), new Circle()]
  let sum = 0
  let caught = 0
  for (let i = -1; i < 7; i++) {
    for (const s of shapes) {
      try {
        sum += s.area(i)
      } catch (e) {
        caught++
        sum += checked(i + 10)
      }
    }
    try {
      sum += apply(checked, i)
    } catch (e) {
      caught++
    }
    const bias = i * 7
    try {
      sum += apply((n: number) => checked(n) + bias, i)
    } catch (e) {
      caught += 10
      sum += checked(i + 20)
    }
  }
  console.log(sum, caught)
}

main()
