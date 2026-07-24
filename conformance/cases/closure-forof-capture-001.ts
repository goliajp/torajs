// escape-capture pre-pass: a closure created inside a for-of body
// must mark its captured outer bindings — the stmt walker skipped
// Stmt::ForOf entirely, so the captured let stack-alloc'd and the
// escaping closure held a dangling slot (SIGBUS once the inliner
// couldn't mask it).
function countViaEscaped(): number {
  let count = 0;
  const fns: (() => void)[] = [];
  for (const x of [1, 2, 3]) {
    fns.push(() => {
      count = count + 1;
    });
  }
  fns[0]();
  fns[1]();
  fns[2]();
  return count;
}
console.log(countViaEscaped());

function joinViaEscaped(): string {
  let acc = "";
  const fns: (() => void)[] = [];
  for (const s of "ab") {
    fns.push(() => {
      acc = acc + s;
    });
  }
  fns[0]();
  fns[1]();
  return acc;
}
console.log(joinViaEscaped());
