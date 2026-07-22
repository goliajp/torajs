// rotation 191 — Promise .then chain propagates cb1's f64 return
// through to cb2's un-annotated param. Before the fix the cb2
// param defaulted to `any` via `desugar_lifted_closure_fn`, the
// pthunk pre-scan skipped it (its param-face gate excluded the
// `any` annotation), and the runtime dispatcher handed the raw
// i64 slot to a cb whose Any param decoded it as a raw integer.
// User saw the f64 bit pattern (0x4025000000000000 = 4622100592565682000)
// instead of 10.5.
//
// Repro shapes covered:
//   A — literal f64 cb1 return
//   B — arithmetic-produced f64 (n * 10, n / 2)
//   C — identity chain n=>n (checker seeds cb1's param from the
//       receiver's Promise inner, sniff then propagates)
//   D — integer-only chain stays i64 (n + 1 doesn't widen)

async function main() {
  await Promise.resolve(1)
    .then(n => 10.5)
    .then(n => {
      console.log('A:', n)
    })

  await Promise.resolve(1)
    .then(n => n * 10)
    .then(n => {
      console.log('B1:', n)
    })

  await Promise.resolve(4)
    .then(n => n / 2)
    .then(n => {
      console.log('B2:', n)
    })

  await Promise.resolve(10.5)
    .then(n => n)
    .then(n => {
      console.log('C:', n)
    })

  await Promise.resolve(1)
    .then(n => n + 1)
    .then(n => {
      console.log('D:', n)
    })
}
main()
