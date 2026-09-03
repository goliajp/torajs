// §14.8.1 / §14.9.1 — the legal half of the break/continue label rules.
// The illegal half is a parse-phase SyntaxError and so cannot live in
// the same file; each rejected shape is listed here as a comment and
// checked by hand against bun (all five reject in both):
//   break nonexist            undefined label
//   continue nonexist         undefined label
//   lbl: { continue lbl }     label does not label a loop
//   sw: switch(1){case 1: continue sw}   same, via a switch
//   break; / continue;        outside any loop (or switch, for break)
// The point of the file is that the gate deciding those does not
// reject anything here.

let log: string[] = [];

outer: for (let i = 0; i < 3; i++) {
  if (i === 1) continue outer;
  if (i === 2) break outer;
  log.push("loop" + i);
}

// two labels on one loop — `break a` targets it through the stack
a: b: for (let i = 0; i < 2; i++) {
  log.push("ab" + i);
  break a;
}

// a labelled BLOCK is a legal break target and an illegal continue one
blk: {
  log.push("in-block");
  break blk;
}

// a labelled block nested inside a labelled loop: `continue` still
// finds the loop's label through it
c: for (let i = 0; i < 2; i++) {
  inner: {
    if (i === 0) continue c;
    log.push("through-block");
  }
}

// bare `break` inside a switch, and a labelled `continue` that leaves
// a switch to reach its loop
d: for (let i = 0; i < 3; i++) {
  switch (i) {
    case 0:
      log.push("sw0");
      break;
    case 1:
      continue d;
    default:
      log.push("sw" + i);
  }
}

// break / continue crossing a try..finally
e: for (let i = 0; i < 2; i++) {
  try {
    if (i === 0) continue e;
    break e;
  } finally {
    log.push("fin" + i);
  }
}

// a function body starts from an empty label set, so its own labels
// are the only ones it can name
function ownLabels() {
  let n = 0;
  f: for (let i = 0; i < 3; i++) {
    if (i === 1) continue f;
    n += i;
  }
  return n;
}

// and the same for an arrow body
const arrowLabels = () => {
  g: for (let i = 0; i < 2; i++) {
    break g;
  }
  return "arrow";
};

class WithLoops {
  m() {
    h: for (let i = 0; i < 2; i++) {
      break h;
    }
    return "method";
  }
}

do {
  log.push("do");
  break;
} while (false);

for (const x of [1, 2]) {
  if (x === 1) continue;
  log.push("forof" + x);
}

console.log(log.join(","));
console.log(ownLabels(), arrowLabels(), new WithLoops().m());
