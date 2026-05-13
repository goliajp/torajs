// V3-18 wedge — flow-sensitive narrowing on `<ident> !== null`
// / `<ident> === null` cond shapes. Per TS spec §3.6.5 the
// narrowing is the most common idiom for guarding access to
// possibly-null fields (linked-list walks, optional state
// fields, etc). Pre-fix tora rejected with 'no member .x on
// type Nullable<...>' inside the guarded branch since the
// declared type stayed Nullable.
//
// Implementation: in the Stmt::If handler, detect the cond
// shape (BinOp Neq/LooseNeq/Eq/LooseEq with `<Ident>` and
// `Null`), look up the binding's Nullable<T> type, override
// to T inside the matching branch (then for !==, else for
// ===), restore after. Subset limitation: only single-ident
// targets are narrowed (no `<member>` paths like `o.field
// !== null`); the cond must be the entire If condition (no
// `&&` / `||` composition); while-loop conditions are not
// yet plumbed.

type O = { x: number }
let o: O | null = { x: 5 }
if (o !== null) {
  console.log(o.x * 2)                 // 10
}

// Loose != also narrows.
let p: O | null = { x: 7 }
if (p != null) {
  console.log(p.x)                     // 7
}

// === null → narrow in the else branch.
let q: O | null = { x: 99 }
if (q === null) {
  console.log("absent")
} else {
  console.log(q.x)                     // 99
}

// Linked-list head walk (the canonical use site). Note: while
// loops aren't narrowed yet; user must add `!` inside.
class Node {
  constructor(public v: number, public next: Node | null = null) {}
}
let head = new Node(1, new Node(2, new Node(3, null)))
if (head !== null) {
  console.log(head.v)                  // 1 — narrowed access
}

// Function-param flow narrowing.
function nameOf(o: O | null): string {
  if (o !== null) {
    return "x=" + o.x.toString()
  }
  return "(none)"
}
console.log(nameOf({ x: 42 }))         // x=42
console.log(nameOf(null))              // (none)

// Post-if narrowing when one branch diverges (the dominant
// real-world pattern: early-return on null leaves the rest
// of the fn body with the binding narrowed to T).
function safeAccess(o: { v: number } | null): number {
  if (o === null) {
    return -1
  }
  return o.v + 1                       // narrowed by the early return
}
console.log(safeAccess({ v: 10 }))     // 11
console.log(safeAccess(null))          // -1
