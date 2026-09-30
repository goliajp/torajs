// The loop writes `re.lastIndex` between execs: past the end, back to
// the start, and a fractional value.

function run(s: string): void {
  const re = /(\d+)/g;
  let m = re.exec(s);
  let n = 0;
  while (m !== null) {
    n = n + 1;
    console.log(n, m.index, m[1], re.lastIndex);
    if (n === 1) {
      re.lastIndex = 1.5;
    } else if (n === 3) {
      re.lastIndex = 100;
    } else if (n === 4) {
      re.lastIndex = 0;
    }
    if (n > 6) {
      break;
    }
    m = re.exec(s);
  }
  console.log("done", re.lastIndex);
}
run("a1b22c333");
