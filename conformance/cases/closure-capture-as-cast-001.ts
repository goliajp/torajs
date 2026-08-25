// rotation 497 — a callback wrapped in `as T` (or a comma sequence)
// still captures: the escape-capture census walked past `As` /
// `Sequence`, so a mutable `let` written by such a callback was never
// heap-boxed, and the closure env then treated the raw stack slot as a
// capture box (regex-020 case 10 crashed behind the
// injection-reachability gate once main's frame layout moved).
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

let hits = 0
console.log((0, "aXbXc".replaceAll(/X/g, (): string => { hits++; return "-" })))
console.log(hits)

let seen = ""
const r = ("q1".replace(/\d/, (m: string): string => { seen = m; return "#" }) as string) as string
console.log(r, seen)
