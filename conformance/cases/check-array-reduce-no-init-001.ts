// ES §23.1.3.24 / §23.1.3.25 — `xs.reduce(cb)` / `xs.reduceRight(cb)`
// 1-arg overload. Initial value defaults to `arr[0]` (or `arr[len-1]`
// for reduceRight); the loop cursor starts one slot in. Empty arr
// throws TypeError per spec — MVP elides the guard.

const ns: number[] = [1, 2, 3, 4, 5]

// number reduce — sum
console.log('reduce sum', ns.reduce((a: number, b: number) => a + b))

// number reduceRight — concatenate as math (commutative, same sum)
console.log('reduceRight sum', ns.reduceRight((a: number, b: number) => a + b))

// number reduce — max via Math.max
console.log('reduce max', ns.reduce((a: number, b: number) => (a > b ? a : b)))

// number reduce — min via Math.min
console.log('reduce min', ns.reduce((a: number, b: number) => (a < b ? a : b)))

// non-commutative reduce — subtract: ((((1-2)-3)-4)-5) = -13
console.log('reduce sub', ns.reduce((a: number, b: number) => a - b))

// reduceRight non-commutative — subtract right-to-left: ((((5-4)-3)-2)-1) = -5
console.log('reduceRight sub', ns.reduceRight((a: number, b: number) => a - b))

// string reduce — concat (refcounted elem path)
const ss: string[] = ['alpha', 'bravo', 'charlie']
console.log('str reduce', ss.reduce((a: string, b: string) => a + '-' + b))

// string reduceRight — reverse-order concat
console.log('str reduceRight', ss.reduceRight((a: string, b: string) => a + '-' + b))

// single-element array — callback never fires, returns arr[0]
console.log('single reduce', [42].reduce((a: number, b: number) => a + b))
console.log('single reduceRight', [42].reduceRight((a: number, b: number) => a + b))

// 2-arg form still works (no regression on existing path).
console.log('2-arg sum', ns.reduce((a: number, b: number) => a + b, 100))
console.log('2-arg right sum', ns.reduceRight((a: number, b: number) => a + b, 100))
