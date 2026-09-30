// `/y` — each exec must match exactly at lastIndex; a miss resets it.

function run(s: string): void {
  const re = /(\w)(\d)/y;
  let m = re.exec(s);
  let n = 0;
  while (m !== null) {
    n = n + 1;
    console.log(n, m.index, m[1], m[2].length, re.lastIndex);
    m = re.exec(s);
  }
  console.log("end", re.lastIndex, m === null);
}
run("a1b2c3");
run("a1 b2");
run("");
