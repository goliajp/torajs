// A null guard narrows the reference it tests for the statement it
// guards, and a write inside that statement retires the narrow where it
// happens: the right-hand side still reads under it, the uses after it
// see `T | null` again. `while (t.next !== null) t = t.next` and
// `while (q !== null) { t = q; q = q.next }` were refused at the
// assignment (the narrow was dropped for any body that wrote the
// reference, and a class receiver's field never narrowed at all).

class Node {
  v: number;
  next: Node | null;
  constructor(v: number) {
    this.v = v;
    this.next = null;
  }
}

function chain(n: number): Node {
  const head = new Node(1);
  let t = head;
  for (let i = 2; i <= n; i++) {
    const c = new Node(i);
    t.next = c;
    t = c;
  }
  return head;
}

function v(n: Node): number {
  return n.v;
}

// member path on a class receiver, receiver rewritten in the body
function last(head: Node): number {
  let t = head;
  while (t.next !== null) t = t.next;
  return t.v;
}
console.log("last:", last(chain(5)), last(chain(1)));

// binding guard, the binding rewritten after its use
function lastViaQ(head: Node): number {
  let t: Node = head;
  let q: Node | null = head.next;
  while (q !== null) {
    t = q;
    q = q.next;
  }
  return t.v;
}
console.log("lastViaQ:", lastViaQ(chain(4)), lastViaQ(chain(1)));

// a re-test after the write narrows again
function pairs(head: Node | null): number {
  let q = head;
  let s = 0;
  while (q !== null) {
    s += v(q);
    q = q.next;
    if (q === null) break;
    s += 10 * v(q);
  }
  return s;
}
console.log("pairs:", pairs(chain(5)), pairs(null));

// a write in a branch that cannot fall through leaves the narrow alone
function skipEven(head: Node | null): string {
  let q = head;
  let out = "";
  while (q !== null) {
    if (q.v % 2 === 0) {
      q = q.next;
      continue;
    }
    out += v(q);
    q = q.next;
  }
  return out;
}
console.log("skipEven:", skipEven(chain(7)));

// both branches write: the narrow is gone after the if, a guard brings it back
function stride(head: Node | null, k: number): number {
  let q = head;
  let n = 0;
  while (q !== null) {
    n++;
    if (k > 1) {
      q = q.next;
    } else {
      q = null;
    }
    if (q !== null) n += v(q) * 0;
  }
  return n;
}
console.log("stride:", stride(chain(6), 2), stride(chain(6), 1));

// an object-literal type takes the same path as a class
type S = { v: number; next: S | null };
const s: S = { v: 1, next: { v: 2, next: { v: 3, next: null } } };
let u: S = s;
while (u.next !== null) u = u.next;
console.log("struct:", u.v);

// a write to the guarded member itself
const h = chain(4);
let removed = 0;
while (h.next !== null) {
  removed += h.next.v;
  h.next = h.next.next;
}
console.log("drained:", removed, h.next === null);
