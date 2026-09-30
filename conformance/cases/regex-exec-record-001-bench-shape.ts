// The exec-record lane — a `let m = re.exec(line)` that is only
// null-tested, read as `m.index`, indexed by a constant and measured
// holds the match as spans instead of building the array. The bench
// shape: total of index + group length + parsed group over a loop.

const lines = ["a=1;bb=22;ccc=333;", "=5;x7;dd=44;", "", "zz=9"];
const re = /([a-z]+)=(\d+)/g;
let total = 0;
for (let i = 0; i < 40; i = i + 1) {
  const line = lines[i & 3];
  re.lastIndex = 0;
  let m = re.exec(line);
  while (m !== null) {
    total = total + m.index + m[1].length + parseInt(m[2], 10);
    m = re.exec(line);
  }
}
console.log(total);
