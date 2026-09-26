// A comparator's abrupt completion ends the sort (§23.1.3.30.2 step 3)
// and propagates to the caller. Comparators that miss the native
// kernel's shape (no parameters, mismatched widths) run the inline
// insertion sort; a throw there must neither be dropped nor surface at
// a later, unrelated catch, and the array keeps every element once.
const ys: number[] = [3, 1, 2]
try {
  ys.sort((): number => { throw new Error("zero-arity") })
  console.log("no throw", ys.join(","))
} catch (e) {
  console.log("caught", (e as Error).message)
}
try { throw new Error("later") } catch (e) { console.log("next catch", (e as Error).message) }
const words: string[] = ["delta", "alpha", "charlie", "bravo"]
let n = 0
try {
  words.sort((): number => { n++; if (n === 3) throw new Error("third"); return 1 })
} catch (e) {
  console.log("caught", (e as Error).message, n)
}
console.log(words.slice().sort().join(","), words.length)
