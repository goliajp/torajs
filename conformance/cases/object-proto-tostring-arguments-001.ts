// §20.1.3.6 step 5 — an arguments object has a [[ParameterMap]] and
// takes its own badge. It reaches that badge precisely because step 3
// says it is NOT an array exotic object: tr mints it as an Arr cell,
// so only FLAG_ARR_ARGUMENTS separates the two questions.
function f() {
  console.log(Object.prototype.toString.call(arguments))
  const a: any = arguments
  console.log(a.length, a[0], a[1])
}
f(7, 8)

// A real array is unaffected.
console.log(Object.prototype.toString.call([1, 2]))
console.log(Object.prototype.toString.call(new Proxy([1, 2], {})))

// Zero-argument and spread-built call sites reach the same cell.
function g() {
  console.log(Object.prototype.toString.call(arguments), arguments.length)
}
g()
g(...[1, 2, 3])
