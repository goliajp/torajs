// An arrow inside a named function capturing a PARAMETER that shares
// its name with a top-level binding reads the parameter; the
// top-level binding keeps its own value and its own readers, and an
// arrow capturing a name the function does not declare still reaches
// the top-level one.
function addAll(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
function scaleAll(xs: number[]): number[] {
  return xs.map((x: number): number => x * factor)
}
function nested(k: number): () => number {
  return (): number => {
    const inner = (): number => k + offset
    return inner()
  }
}
const offset = parseInt("100", 10)
const factor = parseInt("3", 10)
const k = parseInt("7", 10)
const xs: number[] = [1, 2, 3]
console.log(addAll(xs, 1), addAll(xs, offset))
console.log(scaleAll(xs).join(","))
console.log(nested(1)(), nested(k)())
console.log(offset, factor, k)
