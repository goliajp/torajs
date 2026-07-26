// P-SURF S8.5 — an `Array<Any>` handed back through a typed `T[]`
// return had to be decoded and was not.
//
// The three-line shape that found it:
//
//   function* s(): number { yield 1; yield 2 }
//   function read(): number[] { return [...s()] }
//   console.log(read());     // [ -562949953421311, -562949953421310 ]
//   console.log([...s()]);   // [ 1, 2 ] — the same spread, printed in place
//
// Those values are NaN-box payloads read as `f64`. Spreading an
// iterated source produces `Array<Any>` (what it yields is not knowable
// statically), the checker admits that into a `number[]` return through
// the assignability lattice, and the caller then reads Any-tagged slots
// raw. The let-decl lane has decoded at this boundary since chunk 698 —
// `const a: number[] = [...s()]` was always right — so the return lane
// was the one paying nothing for the same admission.

function* nums(): number {
  yield 1;
  yield 2;
  yield 3;
}

function* strs(): string {
  yield "a";
  yield "b";
}

// the shape that found it
function readNums(): number[] {
  return [...nums()];
}

// a generator with no annotation yields `any`, so the elements really
// are erased and the declared return is what says how to read them
function* loose() {
  yield 10;
  yield 20;
}
function readLoose(): number[] {
  return [...loose()];
}

// strings take the same route
function readStrs(): string[] {
  return [...strs()];
}

// spread mixed with literal elements in one array
function readMixed(): number[] {
  return [0, ...nums(), 99];
}

// two spreads of the same generator, each freshly started
function readTwice(): number[] {
  return [...nums(), ...nums()];
}

// a Map / Set iterator is the same erased-element source
function readSet(): number[] {
  const s = new Set<number>();
  s.add(4);
  s.add(5);
  return [...s];
}
function readMapKeys(): string[] {
  const m = new Map<string, number>();
  m.set("k", 1);
  m.set("j", 2);
  return [...m.keys()];
}

// an arrow with a declared return type goes through the same lane
const readArrow = (): number[] => {
  return [...nums()];
};

// a class method too
class Reader {
  read(): number[] {
    return [...nums()];
  }
}

console.log(readNums(), readLoose(), readStrs());
console.log(readMixed(), readTwice());
console.log(readSet(), readMapKeys());
console.log(readArrow(), new Reader().read());

// reading an element back individually, not just printing the array —
// this is where a raw NaN-box slot showed as a huge negative integer
const got = readNums();
console.log(got[0] + got[1] + got[2], got.length);
const words = readStrs();
console.log(words[0] + words[1]);

// still open, and deliberately not exercised above: a *borrowed* return
// (`const a = [...nums()]; return a`) and a nested one
// (`return [[...nums()]]`). The first is gated off because the shared
// helper refuses to copy an aliasable source; the second because the
// runtime decode walks one level. Both remain filed under S8.5.
