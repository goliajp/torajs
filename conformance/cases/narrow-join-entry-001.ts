// Narrows where control arrives from more than one place, in the
// shapes that stay legal: a switch clause entered from the head and
// from the clause above, a do..while condition reached by `continue`,
// and a closure created under a narrow.

class Cell {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function make(n: number): Cell | null {
  return n > 0 ? new Cell(n) : null;
}

function read(c: Cell): number {
  return c.v;
}

// every way into `case 2` agrees: the head (narrowed before the switch)
// and `case 1` falling through
function viaHead(n: number, k: number): number {
  const c = make(n);
  if (c === null) return -1;
  let s = 0;
  switch (k) {
    case 1:
      s += 10;
    case 2:
      s += read(c);
      break;
    default:
      s = 0;
  }
  return s;
}
console.log("head:", viaHead(0, 1), viaHead(3, 1), viaHead(3, 2), viaHead(3, 5));

// a narrow made inside a clause holds for the rest of that clause
function inClause(n: number, k: number): number {
  let c = make(n);
  switch (k) {
    case 1:
      if (c === null) c = new Cell(40);
      return read(c);
    case 2:
      if (c === null) return -2;
      return read(c) * 2;
    default:
      return c === null ? -3 : 0;
  }
}
console.log("clause:", inClause(0, 1), inClause(2, 1), inClause(0, 2), inClause(2, 2), inClause(0, 9));

// both clauses that reach `case 2` fill the binding: the head through
// the default above it, and `case 1`
function bothFill(n: number, k: number): number {
  let c = make(n);
  switch (k) {
    case 1:
      if (c === null) c = new Cell(5);
    default:
      if (c === null) c = new Cell(6);
    case 2:
      return c === null ? -1 : read(c);
  }
}
console.log("both:", bothFill(0, 1), bothFill(0, 2), bothFill(0, 7), bothFill(8, 2));

// the condition after a body that leaves on null instead of continuing
function doLoop(n: number, k: number): number {
  const c = make(n);
  let s = 0;
  do {
    if (c === null) return -1;
    s++;
  } while (read(c) > s && --k > 0);
  return s;
}
console.log("do:", doLoop(0, 3), doLoop(5, 3), doLoop(2, 9));

// `continue` with the binding possibly null: the condition does not
// read it
function doContinue(n: number, k: number): number {
  const c = make(n);
  let s = 0;
  do {
    if (c === null) continue;
    s += read(c);
  } while (--k > 0);
  return s;
}
console.log("do-continue:", doContinue(0, 3), doContinue(9, 3));

// a closure created under a narrow nothing writes again keeps it
function constClosure(n: number): number {
  const c = make(n);
  if (c === null) return -1;
  const f = (): number => read(c) + 1;
  return f();
}
console.log("closure:", constClosure(0), constClosure(4));

// a closure that does not need the narrow sees the later write
function seesWrite(n: number): string {
  let c = make(n);
  if (c === null) c = new Cell(1);
  const isNull = (): boolean => c === null;
  const before = isNull();
  c = null;
  return `${before} ${isNull()}`;
}
console.log("sees:", seesWrite(0), seesWrite(3));
