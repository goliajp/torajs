// An unannotated binding initialized with an array literal lends the
// literal's element type to the parameters of a callback on it only
// when every element names the same type: a list of [string, bigint]
// pairs is not a string[][], and a destructuring callback parameter
// that shadows the list's own name still reads each pair whole.

const expected = [["hour", 1n], ["minute", 2n]]
expected.forEach(([unit, expected]) => {
  console.log(unit, expected)
})
const mixed = [["a", 1], ["b", "two"]]
mixed.forEach(([k, v]) => console.log(k, v))
const words = [["x", "y"], ["z", "w"]]
words.forEach(([a, b]) => console.log(a + b))
