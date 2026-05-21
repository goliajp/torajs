// P9.5-A1.1 — String.prototype.replace(re, fn) with capture-group
// spread per ES spec §22.1.3.18. cb signature is
// `(match, g1, g2, ..., gN) => string` where N = number of capture
// groups in the regex literal (counted statically at ssa-lower).
//
// A1.1 scope: N ∈ [0, 9]. Non-participating groups (e.g. `(a)|(b)`
// where only one alternative fires) emit empty Str rather than
// `undefined` (Nullable<Str> cb params = A1.1.1 follow-up).
//
// All cases byte-equal vs bun (manually verified).

// 1. N=1 — single capture group, single match
console.log("a1b2c3".replace(/(\d)/g, (_m: string, d: string): string => `(${d})`))

// 2. N=1 — quantifier inside capture
console.log("hello".replace(/(l+)/, (_m: string, run: string): string => `[${run}]`))

// 3. N=2 — canonical "swap" idiom from bun docs
console.log(
    "Jane Doe".replace(
        /(\w+) (\w+)/,
        (_m: string, first: string, last: string): string => `${last}, ${first}`,
    ),
)

// 4. N=2 — replaceAll with global, key=value parsing
console.log(
    "a=1,b=2,c=3".replaceAll(
        /(\w+)=(\w+)/g,
        (_m: string, k: string, v: string): string => `${k}:${v}`,
    ),
)

// 5. N=3 — date format split + reorder
console.log(
    "2026-05-21".replace(
        /(\d+)-(\d+)-(\d+)/,
        (_m: string, y: string, mo: string, d: string): string => `${y}/${mo}/${d}`,
    ),
)

// 6. Named capture — cb still positional per spec (groups dict not
//    surfaced as cb arg in v0.1; A1.1 narrow scope)
console.log(
    "k=v".replace(
        /(?<k>\w+)=(?<v>\w+)/,
        (_m: string, k: string, v: string): string => `${v}=${k}`,
    ),
)

// 7. N=1 with non-capturing group adjacent — cb gets 1 cap (not 2)
console.log(
    "a-b-c".replace(/(\w+)(?:-)/, (_m: string, x: string): string => `[${x}]`),
)

// 8. N=2 with closure capture in cb body
let pairCount = 0
console.log(
    "k1=v1,k2=v2,k3=v3".replaceAll(
        /(\w+)=(\w+)/g,
        (_m: string, k: string, v: string): string => {
            pairCount = pairCount + 1
            return `${pairCount}:${k}->${v}`
        },
    ),
)
console.log(pairCount)

// 9. N=1 — empty capture (zero-width match advance)
console.log("abc".replace(/(b*)/g, (_m: string, x: string): string => `<${x}>`))

// 10. N=1 — sticky + capture
console.log(
    "a1a2a3X".replace(/(a)(\d)/gy, (_m: string, a: string, d: string): string => `${a}.${d}`),
)

// 11. N=3 — nested groups: outer + inner
console.log(
    "abc123".replace(
        /((a)(b))/,
        (_m: string, outer: string, a: string, b: string): string => `${outer}|${a}|${b}`,
    ),
)

// 12. N=2 — used to compute new value via numeric coercion
console.log(
    "x*5+y*3".replaceAll(
        /(\w)\*(\d)/g,
        (_m: string, name: string, num: string): string =>
            `${name}=${Number(num) * 2}`,
    ),
)

// 13. N=1 — no match: cb never fires
console.log("abc".replace(/(\d)/, (_m: string, d: string): string => `[${d}]`))

// 14. N=2 — alternation INSIDE a capture group: `(a|b)(\d)` — both
//     groups always participate. (A1.1 narrow scope: non-participating
//     groups currently emit empty Str rather than `undefined`. A
//     test for that divergence requires Nullable<Str> cb params —
//     deferred to A1.1.1 follow-up. Fixture stays byte-equal vs bun.)
console.log(
    "a1b2".replaceAll(
        /(a|b)(\d)/g,
        (_m: string, ch: string, d: string): string => `[${ch}:${d}]`,
    ),
)
