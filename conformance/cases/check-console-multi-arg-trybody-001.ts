// B134 follow-up — multi-arg `console.log(arg, typedArr)` inside a
// try-body. Pre-fix the per-arg inspect dispatch lived only in
// `lower_top_stmt`; statements inside `try { ... } catch ...` route
// through `lower_stmt::Stmt::Expr` instead, falling onto the legacy
// `coerce_to_str` joiner that panics on typed `Arr<T>` args.

const ns: number[] = [1, 2, 3, 4, 5]
try {
  console.log('ns', ns)
  console.log('fill(0)', ns.fill(0))
  console.log('toSpliced', ns.toSpliced(1, 2))
} catch (e: any) {
  console.log('caught', e.message)
}

// nested block — same code path, lower_stmt walks the block body.
{
  const ss: string[] = ['alpha', 'bravo', 'charlie']
  console.log('ss in block', ss)
  console.log('two arrs', ns, ss)
}

// boolean / mixed primitive — exercise the typed-walker arms.
const bs: boolean[] = [true, false, true]
try {
  console.log('bs', bs)
  console.log('count=', 3, 'flags=', bs)
} catch (e: any) {
  console.log('caught', e.message)
}
