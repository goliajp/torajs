// W4 follow-up (②.7) — JSON.parse number-face width. JSON text is
// runtime data: no static analysis proves a number-domain face fed
// by JSON.parse stays integral, and the typed cursor parser made a
// narrow face WORSE than a bit-pun — parse_int consumed `2` of
// `2.5` and left the cursor on `.5`, deranging every later field
// (the nested shape below produced zero output, rc=134, pre-fix).
// Every number-domain face of the parse target now seeds F64; the
// array push passes f64 BITS through the raw slot (pre-fix it
// fptosi'd and the slot read back denormals).

// scalar (the long-standing T-02 promotion — anchor).
let n: number = JSON.parse("2.5");
console.log(n);  // 2.5

// array elems.
let xs: number[] = JSON.parse("[1.5, 2]");
console.log(xs[0], xs[1]);  // 1.5 2

// named-type field (pre-fix: 2.5 silently read back as 2).
type P = { x: number };
let p: P = JSON.parse('{"x": 2.5}');
console.log(p.x);  // 2.5

// fractional field followed by more fields — the cursor-derangement
// shape (pre-fix: silent SIGABRT, zero output).
type Q = { x: number, tags: string[] };
let q: Q = JSON.parse('{"x": 2.5, "tags": ["a", "b"]}');
console.log(q.x, q.tags[1]);  // 2.5 b

// nested array field.
type R = { xs: number[] };
let r: R = JSON.parse('{"xs": [1.5, 2.5]}');
console.log(r.xs[0], r.xs[1]);  // 1.5 2.5

// integral data stays correct through the widened faces.
let ys: number[] = JSON.parse("[1, 2]");
console.log(ys[0], ys[1]);  // 1 2
type S = { n: number };
let s: S = JSON.parse('{"n": 7}');
console.log(s.n);  // 7
