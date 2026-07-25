// A function can hand back a sentinel on purpose — `return xs.find(...)`
// passes along whatever a miss answered — and the call site has to route
// it the same way it routes a function that ran off its end.
//
// Both reasons now put the callee on one table. Before that a `number`
// pass-through printed NaN and a `string` one answered typeof "string",
// because the consuming end only recognised the fall-through reason.

const nums: number[] = [1];
const strs: string[] = ["a"];

function findNum(): number {
  return nums.find((v) => v > 9);
}

function findStr(): string {
  return strs.find((v) => v > "z");
}

function popEmpty(): string {
  const empty: string[] = [];
  return empty.pop();
}

function atPast(): number {
  return nums.at(5);
}

// the return is nested rather than the body's last statement
function nested(flag: boolean): number {
  if (flag) {
    return 1;
  } else {
    return nums.find((v) => v > 9);
  }
}

console.log(findNum());
console.log(findStr());
console.log(popEmpty());
console.log(atPast());
console.log(nested(false), nested(true));

console.log(findNum() === undefined);
console.log(typeof findNum());
console.log(findStr() === undefined);
console.log(typeof findStr());

// a hit passes through as itself, and so does a plain value
function findHit(): number {
  return nums.find((v) => v > 0);
}
console.log(findHit(), findHit() === undefined);

function plain(): number {
  return 0;
}
console.log(plain(), plain() === undefined);
