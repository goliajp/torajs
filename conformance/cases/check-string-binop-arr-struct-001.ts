// `String + Arr` / `String + Struct` per ES §13.15.3 — non-string side
// ToPrimitive(Default) → ToString. Mirror of the explicit `String(arr)`
// S137 dispatch (arr_join("," ) for Arr, "[object Object]" for Obj).
console.log('arr=' + [1, 2, 3])
console.log('floats=' + [1.5, 2.5, 3.5])
console.log('strs=' + ['a', 'b', 'c'])
console.log('bools=' + [true, false, true])
console.log('empty=' + [])

// Reverse direction (Arr / Obj on left).
console.log([1, 2, 3] + ' tail')
console.log({a: 1} + ' done')

// Struct concat both directions.
console.log('o=' + {name: 'alice'})
console.log({x: 1, y: 2} + ' done')

// Substr + Arr (Substr produced by indexing into a Str).
const tag = 'xs'
console.log(tag + ':' + [10, 20])

// Compound with chained concat.
const xs = [1, 2, 3]
console.log('xs has ' + xs.length + ' items: ' + xs)
