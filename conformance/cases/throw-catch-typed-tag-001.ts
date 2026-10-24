// A typed catch binding admits only a value of its type.
//
// `catch (e: T)` with a T other than `any` / `unknown` is no
// TypeScript (TS1196), so bun has nothing to say about it; tr takes the
// annotation as the slot's type. The pending-throw slot is shared by
// every raise in the program, and a `try` body can raise what none of
// its throw sites wrote: a member read off `undefined` raises a
// TypeError cell. The binding reads the slot's tag first, and a value
// of another type is a TypeError where the binding is made, routed
// like any throw out of the catch. Before the check the binding read
// the cell's address as the number.

class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

class Other {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

class Sub extends Fail {}

function readsUndefined(): number {
  const o: any = []
  return o.x.y
}

function throwsOther(n: number): number {
  if (n > 0) throw new Other(n)
  return n
}

function throwsSub(n: number): number {
  if (n > 0) throw new Sub(n)
  return n
}

function throwsAny(v: any): number {
  if (v !== null) throw v
  return 0
}

function report(outer: any): string {
  return "rejected " + (outer instanceof TypeError) + " " + outer.message
}

function numberGetsError(): string {
  try {
    try {
      readsUndefined()
    } catch (e: number) {
      return "bound " + (e + 1)
    }
  } catch (outer: any) {
    return report(outer)
  }
  return "none"
}

function classGetsError(): string {
  try {
    try {
      readsUndefined()
    } catch (e: Fail) {
      return "bound " + e.code
    }
  } catch (outer: any) {
    return report(outer)
  }
  return "none"
}

function classGetsOtherClass(): string {
  try {
    try {
      throwsOther(7)
    } catch (e: Fail) {
      return "bound " + e.code
    }
  } catch (outer: any) {
    return report(outer)
  }
  return "none"
}

function classGetsSubclass(): string {
  try {
    throwsSub(8)
  } catch (e: Fail) {
    return "bound " + e.code
  }
  return "none"
}

function stringGetsNumber(): string {
  try {
    try {
      throwsAny(3)
    } catch (e: string) {
      return "bound " + e.length
    }
  } catch (outer: any) {
    return report(outer)
  }
  return "none"
}

function numberGetsBoxedNumber(): string {
  try {
    throwsAny(2.5)
  } catch (e: number) {
    return "bound " + (e * 2)
  }
  return "none"
}

let finals = 0
function rejectionRunsFinally(): string {
  try {
    try {
      readsUndefined()
    } catch (e: number) {
      return "bound " + e
    } finally {
      finals = finals + 1
    }
  } catch (outer: any) {
    return report(outer) + " finals=" + finals
  }
  return "none"
}

function unreadBindsNothing(): string {
  let hit = false
  try {
    readsUndefined()
  } catch (e: number) {
    hit = true
  }
  return "unread " + hit
}

console.log(numberGetsError())
console.log(classGetsError())
console.log(classGetsOtherClass())
console.log(classGetsSubclass())
console.log(stringGetsNumber())
console.log(numberGetsBoxedNumber())
console.log(rejectionRunsFinally())
console.log(unreadBindsNothing())
