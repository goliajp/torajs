// A `let` declared in one case clause is one binding for the whole
// CaseBlock, and it stays in its dead zone until control runs through
// the declaration. Entering at a later clause skips it — a read or a
// write there throws ReferenceError; falling through from the
// declaring clause runs it. tr answered the write's value, and read
// the slot before anything was stored in it.
function r(f: () => string): string {
  try {
    return f();
  } catch (e) {
    return (e as Error).constructor.name;
  }
}

function write(x: number): string {
  switch (x) {
    case 1:
      let s = 1;
      s++;
      return "one" + s;
    case 2:
      s = 5;
      return "two" + s;
  }
  return "none";
}
function fall(x: number): string {
  switch (x) {
    case 1:
      let s = 1;
    case 2:
      s = s + 4;
      return "fall" + s;
  }
  return "none";
}
function early(x: number): string {
  switch (x) {
    case 1:
      return "one" + typeof u;
    case 2:
      let u = 3;
      return "two" + u;
  }
  return "none";
}
console.log(r(() => write(1)), r(() => write(2)));
console.log(r(() => fall(1)), r(() => fall(2)));
console.log(r(() => early(1)), r(() => early(2)));

// Every execution of the switch starts the binding over.
const seen: string[] = [];
for (const x of [1, 2, 1, 2]) {
  seen.push(r(() => fall(x)));
}
console.log(seen.join(" "));

// A closure in an earlier clause captures the binding a later clause
// declares.
function capture(x: number): string {
  let g: () => number = () => 0;
  switch (x) {
    case 1:
      g = () => s;
    case 2:
      let s = 7;
      return "c" + g();
  }
  return "none";
}
function captureEarly(x: number): string {
  let g: () => number = () => 0;
  switch (x) {
    case 1:
      g = () => s;
      return "e" + g();
    case 2:
      let s = 7;
      return "c" + g();
  }
  return "none";
}
console.log(r(() => capture(1)), r(() => capture(2)));
console.log(r(() => captureEarly(1)), r(() => captureEarly(2)));

// A default written between cases falls into the case after it.
function mid(x: number): string {
  switch (x) {
    case 1:
      return "one" + s;
    default:
      let s = "d";
    case 3:
      return "three" + s;
  }
}
console.log(r(() => mid(1)), r(() => mid(9)), r(() => mid(3)));

// An inner block's own binding of the name is not the CaseBlock's.
function inner(x: number): string {
  switch (x) {
    case 1:
      let s = 1;
      return "a" + s;
    case 2: {
      let s = 9;
      return "b" + s;
    }
  }
  return "none";
}
console.log(inner(1), inner(2));

// The same inside a generator's machine body.
function* gen(x: number) {
  switch (x) {
    case 1:
      let s = 1;
      yield s;
      return;
    case 2:
      yield s;
  }
}
console.log(r(() => [...gen(1)].join(",")), r(() => [...gen(2)].join(",")));
