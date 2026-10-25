// `x instanceof y` throws a TypeError when `y` is not callable
// (§13.10.2 InstanceofOperator step 4). A function whose only raise
// is that check is a function that may throw: the caller's catch
// receives the TypeError, and a catch that reads its parameter as a
// number sees a value that is not the number thrown.

function probe(x: any, y: any): boolean {
  return x instanceof y
}

function guarded(n: number, y: any): number {
  try {
    if (n < 0) throw n
    probe({}, y)
    return 1
  } catch (e) {
    return (e as number) === -5 ? 100 : 200
  }
}

function named(y: any): string {
  try {
    probe({}, y)
    return "no throw"
  } catch (e) {
    return "caught " + (e instanceof TypeError)
  }
}

const cases: any[] = [Object, 3]
for (const y of cases) {
  console.log(named(y))
  console.log(guarded(1, y))
}
console.log(guarded(-5, Object))
