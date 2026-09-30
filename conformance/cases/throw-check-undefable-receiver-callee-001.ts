// A callee whose only raise is a member read off an element that may
// be undefined (`xs[i].kind` with `i` past the end): the read's null
// guard records a TypeError, so the callee must count as may-throw and
// its caller's try must catch it at the call — not answer the return
// sentinel and let the throw surface at exit. Acceptance: byte-equal
// with bun.

class Token {
  kind: number;
  constructor(kind: number) {
    this.kind = kind;
  }
}

function kindAt(xs: Token[], i: number): number {
  return xs[i].kind;
}

// two levels: the caller of a caller still needs the check
function kindAtTwice(xs: Token[], i: number): number {
  return kindAt(xs, i) * 2;
}

// the raise inside the callee's loop
function sumKinds(xs: Token[], n: number): number {
  let total = 0;
  for (let i = 0; i < n; i = i + 1) {
    total = total + xs[i].kind;
  }
  return total;
}

// guarded by an early return: the read cannot raise
function kindOrMinus(xs: Token[], i: number): number {
  const t = xs[i];
  if (t === undefined) return -1;
  return t.kind;
}

const toks = [new Token(3), new Token(5)];

function probe(label: string, f: () => number): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

probe("in range", () => kindAt(toks, 1));
probe("past end", () => kindAt(toks, 7));
probe("twice in range", () => kindAtTwice(toks, 0));
probe("twice past end", () => kindAtTwice(toks, 2));
probe("loop in range", () => sumKinds(toks, 2));
probe("loop past end", () => sumKinds(toks, 3));
probe("guarded in range", () => kindOrMinus(toks, 1));
probe("guarded past end", () => kindOrMinus(toks, 9));
console.log("after");
