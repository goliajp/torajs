// Adapted from test262: try-finally without catch — finally runs on
// fall-through; the throw still propagates to the outer catch.
function f(): number {
  try {
    return 5;
  } finally {
    console.log("f-finally");
  }
}

function check(): number {
  if (f() !== 5) { throw "#1"; }

  // Throw with no catch on inner: finally runs, then propagates.
  let caught: string = "";
  try {
    try {
      throw "x";
    } finally {
      console.log("inner-finally");
    }
  } catch (e: string) {
    caught = e;
  }
  if (caught !== "x") { throw "#2"; }
  return 0;
}
console.log(check());
