// A switch's CaseBlock is ONE lexical scope across every case, but
// `for_each_nested_list` hands each case body out separately -- so a
// per-site carve-out that asks each list on its own says the read in
// `case 2` is not inside any block that rebinds the name, and wraps it
// into the declaration's forwarder.

function t(a: any) {
  return "a=" + a + "/" + typeof (this as any)
}

switch (1) {
  case 1:
    const t = 3
  case 2:
    console.log(t)
}

// the same shape with a nested block per case, which was never wrong
switch (1) {
  case 1: {
    const t = 4
    console.log(t)
    break
  }
}

// and a value use outside the switch still gets the forwarder
const h: any = t
console.log(h(7))
