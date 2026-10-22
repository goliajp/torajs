// The unhandled-rejection reporter renders an Error instance as
// `name: message`, the name resolved through the class prototype
// chain (§20.5.3.2: a subclass prototype owns no `name`, so it is
// Error.prototype's). The rendering goes through a link-judged seam;
// this program constructs a user Error subclass directly, so the
// class registration the name walk reads must stay in the artifact.
//
// stdout + exit code are compared; the stderr line (tr: `Error: load
// failed at 4`) is host output and documented here only.

class AppError extends Error {
  code: number
  constructor(msg: string, code: number) {
    super(msg)
    this.code = code
  }
}

async function load(n: number): Promise<number> {
  if (n > 2) throw new AppError("load failed at " + n, n)
  return n * 10
}

console.log(await load(1))
const e = new AppError("probe", 7)
console.log(e.name, e.message, e.code, e instanceof AppError, e instanceof Error)
await load(process.argv.length + 2)
console.log("never")
