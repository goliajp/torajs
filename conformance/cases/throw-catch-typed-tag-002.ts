// A typed catch binding of an object, array or heap-family type admits
// what the checked cast of an annotated slot admits.
//
// `catch (e: T)` with a T other than `any` / `unknown` is no
// TypeScript (TS1196); tr takes the annotation as the slot's type. A
// declared class is tested by its class tag (-001); every other object
// type, an array type and a heap family named by a tag take the thrown
// value through the cast an annotated `let` gives an `any` value, so a
// runtime TypeError cell is no `Shape` and no `number[]`. A function
// type admits any function object, as a function-typed `let` takes an
// `any` value: no tag tells one signature from another, so the
// parameter's calls adapt to the face of the function stored. A
// written `number` binds at f64 width: an `await` and an `any` throw
// can hand it a fraction.
//
// bun strips the annotation, binds whatever was thrown and fails only
// where the body uses the value, so the `.expected` lines that read
// "rejected" differ from bun: "error into number[]", "string[] into
// number[]", "error into interface", "error into RegExp", "number into
// fn type" and "null into class" (bun prints what the body's use of
// the value gives).

interface Shape {
  code: number
}

class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

function readsUndefined(): number {
  const o: any = []
  return o.x.y
}

function nothing(): Fail | null {
  return null
}

function attempt(label: string, f: () => string): void {
  try {
    console.log(label, f())
  } catch (outer: any) {
    console.log(label, "rejected", outer instanceof TypeError, outer.message)
  }
}

attempt("error into number[]", () => {
  try {
    readsUndefined()
  } catch (e: number[]) {
    return "bound " + e.length
  }
  return "none"
})
attempt("number[] into number[]", () => {
  try {
    throw [1, 2, 3]
  } catch (e: number[]) {
    return "bound " + e.length + " " + e[1]
  }
  return "none"
})
attempt("string[] into number[]", () => {
  try {
    throw ["a", "b"]
  } catch (e: number[]) {
    return "bound " + e[1]
  }
  return "none"
})
attempt("string[] into string[]", () => {
  try {
    throw ["a", "b"]
  } catch (e: string[]) {
    return "bound " + e[1]
  }
  return "none"
})
attempt("any array into number[]", () => {
  try {
    throw JSON.parse("[1,2]")
  } catch (e: number[]) {
    return "bound " + e.length
  }
  return "none"
})
attempt("error into interface", () => {
  try {
    readsUndefined()
  } catch (e: Shape) {
    return "bound " + e.code
  }
  return "none"
})
attempt("literal into interface", () => {
  try {
    throw { code: 7 }
  } catch (e: Shape) {
    return "bound " + e.code
  }
  return "none"
})
attempt("range error into Error", () => {
  try {
    throw new RangeError("rr")
  } catch (e: Error) {
    return "bound " + e.message
  }
  return "none"
})
attempt("error into RegExp", () => {
  try {
    readsUndefined()
  } catch (e: RegExp) {
    return "bound " + e.source
  }
  return "none"
})
attempt("regexp into RegExp", () => {
  try {
    throw /ab/ as any
  } catch (e: RegExp) {
    return "bound " + e.source
  }
  return "none"
})
attempt("map into Map", () => {
  try {
    throw new Map([["a", 1]]) as any
  } catch (e: Map<string, number>) {
    return "bound " + e.size
  }
  return "none"
})
attempt("closure into fn type", () => {
  try {
    throw () => 5
  } catch (e: () => number) {
    return "bound " + e()
  }
  return "none"
})
attempt("closure of another face into fn type", () => {
  try {
    throw (x?: number) => (x ?? 10) + 1
  } catch (e: (x: number) => number) {
    return "bound " + e(4)
  }
  return "none"
})
attempt("builtin into fn type", () => {
  try {
    throw Math.max
  } catch (e: (a: number, b: number) => number) {
    return "bound " + e(2, 9)
  }
  return "none"
})
attempt("number into fn type", () => {
  try {
    throw 7
  } catch (e: () => number) {
    return "bound " + e()
  }
  return "none"
})
attempt("null into class", () => {
  try {
    const q = nothing()
    throw q
  } catch (e: Fail) {
    return "bound " + e.code
  }
  return "none"
})
attempt("any fraction into number", () => {
  try {
    throw JSON.parse("2.5")
  } catch (e: number) {
    return "bound " + (e + 1)
  }
  return "none"
})

async function awaited(): Promise<void> {
  try {
    await Promise.reject(2.5)
  } catch (e: number) {
    console.log("await fraction into number", "bound " + (e + 1))
  }
  try {
    await Promise.reject(3)
  } catch (e: number) {
    console.log("await integer into number", "bound " + (e + 1))
  }
}
await awaited()
