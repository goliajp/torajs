// P9.5-A1.2 — String.prototype.replace(re, fn) with full ES spec
// cb arity per §22.1.3.18: `(m, g1, ..., gN, offset, input) => string`
// where offset = match-start index (number) and input = receiver string.
//
// A1.2 narrow scope: trailing args MUST be `(offset: number, input: string)`
// together. Half-shapes like `(m, offset)` without input or `(m, input)`
// without offset panic at compile time. All cases byte-equal vs bun.

// 1. N=0 + offset + input — basic
console.log(
    "abc".replace(
        /b/,
        (_m: string, off: number, input: string): string => `[${off}@${input}]`,
    ),
)

// 2. N=0 global with offset/input — each match sees its own offset
console.log(
    "abcabc".replaceAll(
        /b/g,
        (_m: string, off: number, _input: string): string => `<${off}>`,
    ),
)

// 3. N=1 + offset + input
console.log(
    "a1b2".replace(
        /(\d)/g,
        (_m: string, d: string, off: number, input: string): string =>
            `${d}@${off}/${input.length}`,
    ),
)

// 4. N=2 + offset + input — canonical full-arity bun idiom
console.log(
    "Jane Doe".replace(
        /(\w+) (\w+)/,
        (_m: string, f: string, l: string, off: number, _i: string): string =>
            `${l},${f}@${off}`,
    ),
)

// 5. N=2 + replaceAll + offset — offset advances per match
console.log(
    "k1=v1,k2=v2".replaceAll(
        /(\w+)=(\w+)/g,
        (_m: string, k: string, v: string, off: number, _i: string): string =>
            `[${off}:${k}->${v}]`,
    ),
)

// 6. N=3 + offset + input — multi-capture full arity
console.log(
    "2026-05-21".replace(
        /(\d+)-(\d+)-(\d+)/,
        (_m: string, y: string, mo: string, d: string, off: number, _i: string): string =>
            `${y}/${mo}/${d}@${off}`,
    ),
)

// 7. input arg used in cb body to compute something
console.log(
    "abc".replace(
        /b/,
        (_m: string, off: number, input: string): string =>
            `before=${input.slice(0, off)} after=${input.slice(off + 1)}`,
    ),
)

// 8. Sticky + offset — offsets follow sticky walk
console.log(
    "aaa".replace(
        /a/gy,
        (_m: string, off: number, _input: string): string => `${off}`,
    ),
)

// 9. Empty-match advance — offset stays consistent with bun
console.log(
    "ab".replace(
        /b*/g,
        (_m: string, off: number, _input: string): string => `<${off}>`,
    ),
)

// 10. N=1 + closure capture + offset
let lastOffset = -1
console.log(
    "x1y2z3".replaceAll(
        /([a-z])(\d)/g,
        (_m: string, ch: string, d: string, off: number, _input: string): string => {
            lastOffset = off
            return `${ch}=${d}`
        },
    ) as string,
)
console.log(lastOffset)
