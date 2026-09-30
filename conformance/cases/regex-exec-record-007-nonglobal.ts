// A non-global, non-sticky regex: every exec searches from 0 and
// lastIndex is never touched.

function run(s: string): void {
  const re = /(\d)(\d)?/;
  re.lastIndex = 3;
  let m = re.exec(s);
  let n = 0;
  while (m !== null && n < 3) {
    n = n + 1;
    console.log(n, m.index, m[0].length, m[1], m[2] === undefined, re.lastIndex);
    m = re.exec(s);
  }
  console.log(m === null, re.lastIndex);
}
run("ab12cd3");
run("abc");
