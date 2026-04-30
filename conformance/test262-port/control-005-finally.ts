// Adapted from test262: language/statements/try/*-finally*.js
// Verifies finally runs on normal return, throw-caught, and re-throw paths.
function f1(): number {
  try {
    return 1;
  } finally {
    console.log("f1-finally");
  }
}

function f2(): number {
  try {
    throw 99;
  } catch (e) {
    return 2;
  } finally {
    console.log("f2-finally");
  }
}

function f3(): number {
  let r: number = 0;
  try {
    try {
      throw 7;
    } finally {
      console.log("f3-inner-finally");
    }
  } catch (e) {
    r = e;
  }
  return r;
}

console.log(f1());
console.log(f2());
console.log(f3());
