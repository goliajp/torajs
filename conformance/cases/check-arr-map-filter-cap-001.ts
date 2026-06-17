// Map / filter dst array now pre-sized to src.len so push_unchecked
// stays within cap. Prior `arr_alloc(0)` + push_unchecked stomped past
// the pool block on multi-elem inputs.
console.log([1, 2, 3, 4, 5].map(x => x + 10))
console.log([1, 2, 3, 4, 5].filter(x => x > 2))
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map(x => x * x))
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10].filter(x => x % 2 === 0))

// Empty + single-elem edge cases.
console.log([].map((x: number) => x))
console.log([42].map(x => x + 1))
console.log([1, 2, 3].filter(x => x > 99))

// String element type.
console.log(['a', 'b', 'c'].map(s => s.toUpperCase()))
console.log(['foo', 'bar', 'baz'].filter(s => s.startsWith('b')))
