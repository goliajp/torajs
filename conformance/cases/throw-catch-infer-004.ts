// A raise the may-throw set does not see, in a function a `try` body
// calls, keeps an unannotated catch `any`.
//
// `h` raises a TypeError (ToNumber of a BigInt) that the program-wide
// may-throw analysis does not count, so no check follows the call to
// it and the error stays pending until the check after `f(0)`. The
// catch must see the TypeError, not a number from `f`'s `throw n`;
// the same holds for a call through a vtable slot and an interface
// slot.

function h(x: any): number {
  return +x
}

function f(n: number): void {
  if (n > 0) throw n
}

class K {
  m(x: any): number {
    return +x
  }
}

class L extends K {
  m(x: any): number {
    return 2
  }
}

interface I {
  m(x: any): number
}

class J implements I {
  m(x: any): number {
    return +x
  }
}

function direct(): string {
  try {
    h(1n)
    f(0)
    return "none"
  } catch (e) {
    return "caught " + ((e as number) > 0)
  }
}

function viaSlot(k: K): string {
  try {
    k.m(1n)
    f(0)
    return "none"
  } catch (e) {
    return "caught " + ((e as number) > 0)
  }
}

function viaFace(i: I): string {
  try {
    i.m(1n)
    f(0)
    return "none"
  } catch (e) {
    return "caught " + ((e as number) > 0)
  }
}

function thrown(n: number): string {
  try {
    f(n)
    return "none"
  } catch (e) {
    return "caught " + ((e as number) > 0)
  }
}

console.log(direct())
console.log(viaSlot(process.argv.length > 9 ? new L() : new K()))
console.log(viaFace(new J()))
console.log(thrown(3))
