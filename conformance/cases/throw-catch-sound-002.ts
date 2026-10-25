// Unary `+` on a BigInt throws a TypeError (§13.5.4 ToNumber step 2).
// A function whose only raise is that conversion may throw: the
// statements after its call in the caller do not run, and the caller's
// catch receives the TypeError, whether it binds `any` or reads its
// parameter as a number.

function h(x: any): number {
  return +x
}

function typedCatch(v: any): number {
  try {
    const r = h(v)
    if (r > 1000) throw r
    return r
  } catch (e) {
    return (e as number) - 1
  }
}

function anyCatch(v: any): string {
  const log: string[] = []
  try {
    log.push("before")
    h(v)
    log.push("after")
  } catch (e: any) {
    log.push("caught " + (e instanceof TypeError))
  }
  return log.join(",")
}

const vals: any[] = [5, 2000, 10n]
for (const v of vals) {
  console.log(anyCatch(v))
  const r = typedCatch(v)
  console.log(typeof r, r === r)
}
