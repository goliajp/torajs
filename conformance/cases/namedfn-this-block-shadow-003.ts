// The receiver-bearing arms of the same rule. The namedfn_recv_cb
// collectors carried a THIRD copy of the whole-name refusal -- a
// `collect_decls_by_name` census that is non-empty for a `const` in
// any block anywhere -- so one block reusing the spelling cost the
// receiver at every store / object-literal / any-init / push site of
// that declaration, program-wide.

function t(a: any) {
  const th = this as any
  return a + "/" + (th === undefined ? "undef" : "held")
}

{
  const t = 3
  console.log(t)
}

// a property store on an `: any` holder
const holder: any = {}
holder.m = t
console.log(holder.m(1))

// an object-literal field, annotated and not
const anyheld: any = { m: t }
console.log(anyheld.m(2))
const lit = { m: t }
console.log(lit.m(3))

// an `: any` binding initialised from the name
const alias: any = t
console.log(alias(4))

// an argument pushed into a props container. The container is a
// BINDING here on purpose: `bag.fns.push(t)` with a member container
// answers `undef` with or without a block, which is a different gap.
const z: any[] = []
z.push(t)
console.log(z[0](5))

// and the block still reads its own binding afterwards
{
  const t = 6
  console.log(t)
}

// a receiver-aware HOF callback with a thisArg
function cb(x: any): boolean {
  const th = this as any
  return th !== undefined && th.z === 1
}
const r: any = { z: 1 }
console.log([11].every(cb, r))
{
  const cb = 9
  console.log(cb)
}
