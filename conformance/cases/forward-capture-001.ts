// A closure that captures an ordinary binding declared LATER in the
// same statement list. Both declarations run before anything calls the
// closure, so there is nothing being read early — but capture
// resolution used to ask the scope as it stood and answer "unknown
// identifier".
//
// Two closures that call each other already worked (a capture box goes
// up ahead of the declaration that fills it). The binding does not have
// to be a closure: what a closure captures is the BINDING (ES §9.1), so
// the box goes up for a plain `const` too. Its slot type is the one
// thing not knowable that early — an un-annotated initializer has no
// type until it lowers, which is after the mint that needs the box — so
// the box goes up provisionally and the declaration corrects it.

// an object, a number, and a string: the three slot shapes the box has
// to end up holding
function readsObject(): void {
  const g = (): number => o.v;
  const o = { v: 3 };
  console.log(g());
}

function readsNumber(): void {
  const f = (): number => k;
  const k = 5;
  console.log(f());
}

function readsString(): void {
  const g = (): string => s;
  const s = "hi";
  console.log(g());
}

// an annotated binding takes the same lane — the annotation is not what
// makes it work, it only settles the type earlier
type Pt = { v: number };
function readsAnnotated(): void {
  const g = (): number => p.v;
  const p: Pt = { v: 7 };
  console.log(g());
}

function readsArray(): void {
  const g = (): number => xs.length;
  const xs = [1, 2, 3];
  console.log(g(), xs[2]);
}

// writing back reaches the same cell: the closure and the binding share
// one box, so the later read sees the write
function writesBack(): void {
  const bump = (): void => {
    n = n + 1;
  };
  let n = 1;
  bump();
  bump();
  console.log(n);
}

// and a write through the ordinary name is visible to the closure
function readsAfterReassign(): void {
  const read = (): number => n;
  let n = 1;
  n = 5;
  console.log(read(), n);
}

// a closure declared between the capturing one and its target, so the
// box has to survive another mint
function mixedList(): void {
  const g = (): number => o.v + h();
  const h = (): number => 1;
  const o = { v: 3 };
  console.log(g());
}

// the capture crosses two closure levels: the inner one is lifted first
// and the outer one carries the box through
function twoDeep(): void {
  const outer = (): number => {
    const inner = (): number => o.v * 2;
    return inner();
  };
  const o = { v: 3 };
  console.log(outer());
}

// a block is its own statement list and runs this on its own
function inBlock(): void {
  {
    const g = (): string => s;
    const s = "in-block";
    console.log(g());
  }
}

// a nested `function` declaration is a closure once it captures, so it
// reaches the same lane from the other direction
function nestedFnForm(): void {
  function g(): number {
    return o.v;
  }
  const o = { v: 3 };
  console.log(g());
}

// unchanged ground: capturing a binding declared EARLIER never needed
// any of this, and must still take the ordinary path
function capturesEarlier(): void {
  const o = { v: 3 };
  const g = (): number => o.v;
  console.log(g());
}

// mutual recursion between two closures — the shape that built the
// machinery this reuses
function mutualClosures(): void {
  const isEven = (n: number): boolean => (n === 0 ? true : isOdd(n - 1));
  const isOdd = (n: number): boolean => (n === 0 ? false : isEven(n - 1));
  console.log(isEven(10), isOdd(7));
}

// many iterations of mint-and-drop: the box holds the object and the
// env holds the box, so this is the reference cycle the collector
// exists for — it must not grow
function churns(): number {
  let total = 0;
  for (let i = 0; i < 2000; i++) {
    const g = (): number => o.v;
    const o = { v: i };
    total = total + g();
  }
  return total;
}

function main(): void {
  readsObject();
  readsNumber();
  readsString();
  readsAnnotated();
  readsArray();
  writesBack();
  readsAfterReassign();
  mixedList();
  twoDeep();
  inBlock();
  nestedFnForm();
  capturesEarlier();
  mutualClosures();
  console.log(churns());
}

main();
