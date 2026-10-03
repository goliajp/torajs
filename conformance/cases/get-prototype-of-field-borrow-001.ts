// Object.getPrototypeOf / Reflect.getPrototypeOf only read their
// argument. A field read (an object's property, or a local of an async
// function, which lives in the function's frame object) is borrowed
// from its owner, so the call must leave the owner's reference alone;
// the owner reads, awaits and releases the value afterwards.

async function later(n: number): Promise<number> {
  await null
  return n
}

async function awaitAfterProto(): Promise<void> {
  const p = later(3)
  console.log(Object.getPrototypeOf(p) === Promise.prototype)
  console.log("awaited", await p)
}

async function settledAfterProto(): Promise<void> {
  const p = (async () => 4)()
  console.log(Reflect.getPrototypeOf(p) === Promise.prototype)
  console.log("awaited", await p)
}

async function fromAsyncReassigned(): Promise<void> {
  let p = Array.fromAsync([0, 1, 2])
  console.log(Object.getPrototypeOf(p) === Promise.prototype)
  p = Array.fromAsync([0, 1, 2], (): number => {
    throw new Error("mapper threw")
  })
  console.log(Object.getPrototypeOf(p) === Promise.prototype)
  try {
    await p
    console.log("not rejected")
  } catch (e) {
    console.log("rejected", (e as Error).message)
  }
}

class Holder {
  xs: number[] = [1, 2, 3]
  m: Map<string, number> = new Map([["a", 1]])
}

function fields(): void {
  const h = new Holder()
  for (let i = 0; i < 3; i++) {
    console.log(Object.getPrototypeOf(h.xs) === Array.prototype)
    console.log(Object.getPrototypeOf(h.m) === Map.prototype)
  }
  console.log(h.xs.join(","), h.m.get("a"))
  const o = { s: [7, 8] }
  console.log(Reflect.getPrototypeOf(o.s) === Array.prototype, o.s.length)
}

async function main(): Promise<void> {
  fields()
  await awaitAfterProto()
  await settledAfterProto()
  await fromAsyncReassigned()
  console.log("done")
}

main()
