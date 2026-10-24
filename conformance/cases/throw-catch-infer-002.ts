// An unannotated catch takes the type its try body throws only when
// nothing else can raise there.
//
// `catch (e)` read only as `(e as Fail)` binds a `Fail` when every
// raise the try body reaches is a `throw` of a Fail. A builtin
// constructor's kernel raises too (`new Array(-1)` a RangeError, `new
// Iterator()` a TypeError), and so does a dynamic `new`, an
// `instanceof` whose right side is no function, in the body or in a
// function it calls, and an `await` of a promise that rejects with
// something else: each of these keeps the parameter `any`, where
// `(e as Fail).code` of a RangeError is `undefined`. Typed as a Fail,
// the binding's check stopped the program at the binding instead.

class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

function fails(n: number): void {
  if (n > 5) throw new Fail(n)
}

function checks(n: number, rhs: any): boolean {
  if (n > 5) throw new Fail(n)
  return n instanceof rhs
}

const ctor: any = 5
const notIterable: any = 5

function array(n: number): void {
  try {
    fails(n)
    console.log(new Array(n - 2).length)
  } catch (e) {
    console.log("array", (e as Fail).code)
  }
}

function regexp(n: number, p: string): void {
  try {
    fails(n)
    console.log(new RegExp(p).source)
  } catch (e) {
    console.log("regexp", (e as Fail).code)
  }
}

function iterator(n: number): void {
  try {
    fails(n)
    console.log(new Iterator())
  } catch (e) {
    console.log("iterator", (e as Fail).code)
  }
}

function arraybuffer(n: number): void {
  try {
    fails(n)
    console.log(new ArrayBuffer(n - 2).byteLength)
  } catch (e) {
    console.log("arraybuffer", (e as Fail).code)
  }
}

function typedarray(n: number): void {
  try {
    fails(n)
    console.log(new Uint8Array(n - 2).length)
  } catch (e) {
    console.log("typedarray", (e as Fail).code)
  }
}

function dynamic(n: number): void {
  try {
    fails(n)
    console.log(new ctor())
  } catch (e) {
    console.log("dynamic", (e as Fail).code)
  }
}

function set(n: number): void {
  try {
    fails(n)
    console.log(new Set(notIterable).size)
  } catch (e) {
    console.log("set", (e as Fail).code)
  }
}

function callee(n: number): void {
  try {
    console.log(checks(n, 5))
  } catch (e) {
    console.log("callee", (e as Fail).code)
  }
}

function body(n: number): void {
  try {
    fails(n)
    console.log(n instanceof ctor)
  } catch (e) {
    console.log("body", (e as Fail).code)
  }
}

function thrown(n: number): void {
  try {
    fails(n)
    console.log("thrown none")
  } catch (e) {
    console.log("thrown", (e as Fail).code)
  }
}

array(1)
regexp(1, "(")
iterator(1)
arraybuffer(1)
typedarray(1)
dynamic(1)
set(1)
callee(1)
body(1)
thrown(9)

const rejected: Promise<number> = new Promise((_res, rej) => rej("no"))
try {
  fails(1)
  await rejected
} catch (e) {
  console.log("await", (e as Fail).code)
}
