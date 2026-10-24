// The exhausted step of a Generator<number> whose yields are all
// integers: nothing else in the program writes a fractional `value`, so
// the slot's width comes from this generator alone.
function* sg(): Generator<number> { yield 1; yield 2 }
const it3 = sg()
it3.next(); it3.next()
const e3 = it3.next()
console.log(e3.value, e3.done, typeof e3.value)
const e4: any = e3
console.log(e4.value)
let it: any = sg()
it.next(); it.next()
let end: any = it.next()
console.log(end.value, end.done)
