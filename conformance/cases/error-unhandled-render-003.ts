// The Error root constructor's own observable steps in a program
// whose only other Error use is a promise rejection: ToString of a
// non-string message (§20.5.1.1 step 3.a), the abrupt ToString of a
// Symbol message (a TypeError, §7.1.17), and the inherited `name`
// read through a typed catch binding. Ends on an unhandled rejection
// with an Error reason.
//
// stdout + exit code are compared; the stderr line (tr: `RangeError:
// last`) is host output and documented here only.
//
// exit-code: 1

const n: any = 42
console.log(new Error(n).message, typeof new Error(n).message)
const s: any = Symbol("s")
try {
  new TypeError(s)
  console.log("no throw")
} catch (e) {
  console.log(e instanceof TypeError)
}
try {
  throw new RangeError("typed")
} catch (e: any) {
  const err: Error = e
  console.log(err.name, err.message)
}
const p = Promise.resolve(1).then((v) => {
  console.log("then", v)
  throw new RangeError("last")
})
console.log("sync end")
