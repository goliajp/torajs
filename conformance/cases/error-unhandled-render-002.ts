// A TypeError the runtime raises (reduce of an empty array with no
// initial value, §23.1.3.24 step 4) rejects an awaited promise and
// nobody handles it. Nothing in the program constructs an Error or
// reads one, so the link may drop the error-class registration; the
// raise then falls back to its preformatted string, and the exit is
// still the unhandled-rejection exit.
//
// stdout + exit code are compared; the stderr line (tr: `error:
// uncaught TypeError: reduce of empty array with no initial value`)
// is host output and documented here only.

async function total(n: number): Promise<number> {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) xs.push(i)
  return xs.reduce((a, b) => a + b)
}

console.log(await total(4))
await total(process.argv.length - 2)
console.log("never")
