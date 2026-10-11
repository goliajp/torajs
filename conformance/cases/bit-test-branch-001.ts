// Branches on `(x & K) === 0` / `!== 0` with K one bit or two bits,
// under each block layout: the then arm next, the else arm next, and
// neither (a loop-header test whose body and exit both sit elsewhere).
// Bit 31 is tested through the mask; a typed field store's guard
// tests the frozen and redefined-field header bits (20 and 31).

const out: number[] = [];

// then arm next
function oneBitThenNext(x: number): void {
  if ((x & 0x20) !== 0) {
    out.push(x);
  } else {
    out.push(-x);
  }
}

function twoBitsThenNext(x: number): void {
  if ((x & 0x208) === 0) {
    out.push(5);
  } else {
    out.push(6);
  }
}

function bit31ThenNext(x: number): void {
  if ((x & 0x80000000) === 0) {
    out.push(1);
  } else {
    out.push(2);
  }
}

function bit31PairThenNext(x: number): void {
  const y = x | 0;
  if ((y & 0x80100000) !== 0) {
    out.push(3);
  } else {
    out.push(4);
  }
}

// else arm next
function oneBitElseNext(xs: number[]): number {
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    if ((xs[i] & 0x40) !== 0) break;
    n = n + 1;
  }
  return n;
}

function twoBitsElseNext(xs: number[]): number {
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    if ((xs[i] & 0x8001) === 0) continue;
    n = n + xs[i];
  }
  return n;
}

// neither arm next
function oneBitNeither(xs: number[]): number {
  let i = 0;
  let v = 0;
  while ((v & 0x200) === 0) {
    v = xs[i];
    i = i + 1;
    if ((v & 1) === 0) {
      continue;
    } else {
      break;
    }
  }
  return i;
}

function twoBitsNeither(xs: number[]): number {
  let i = 0;
  let v = 0;
  while ((v & 0x10010) === 0) {
    v = xs[i];
    i = i + 1;
    if (i < xs.length) {
      continue;
    } else {
      break;
    }
  }
  return i;
}

const xs = [0, 1, 2, 3, 0x20, 0x208, 0x200, 0x10, 0x10000, -1, 0x7fffffff, 0x80000000, 0x80100000, 0xffffffff];
for (const x of xs) {
  oneBitThenNext(x);
  twoBitsThenNext(x);
  bit31ThenNext(x);
  bit31PairThenNext(x);
}
console.log(out.join(","));
console.log(oneBitElseNext([0, 1, 2, 0x40, 3]), oneBitElseNext([1, 2]));
console.log(twoBitsElseNext([1, 2, 0x8000, 4, 0x8001, 6]), twoBitsElseNext([2, 4]));
console.log(oneBitNeither([0, 2, 4, 0x200, 1]), oneBitNeither([0, 2, 7, 0x200]));
console.log(twoBitsNeither([0, 2, 0x10, 6]), twoBitsNeither([0x10000]), twoBitsNeither([2, 4]));

class Cell {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function setCell(c: Cell, v: number): string {
  try {
    c.v = v;
    return "ok " + c.v;
  } catch (e) {
    return "threw " + (e instanceof TypeError);
  }
}

const plain = new Cell(1);
const frozen = Object.freeze(new Cell(2));
console.log(setCell(plain, 4), setCell(frozen, 5), setCell(plain, 7));
