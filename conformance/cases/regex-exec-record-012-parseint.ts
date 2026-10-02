// parseInt over a group read straight off the record: the bench shape,
// a group that did not participate under radix 10 and 36, the radix
// left out / 16 / a 0x prefix, an empty group, leading blanks and a
// sign, a wide subject, and the Number.parseInt spelling.

function run(s: string): void {
  const re = /([a-z]*)()=([^;]*)(;)?/g;
  let m = re.exec(s);
  while (m !== null) {
    console.log(
      m.index,
      parseInt(m[1], 36),
      parseInt(m[2], 10),
      parseInt(m[3], 10),
      parseInt(m[3]),
      parseInt(m[3], 16),
      parseInt(m[4], 10),
      parseInt(m[4], 36),
      Number.parseInt(m[3], 10),
      parseInt(m[3], 10, m.index)
    );
    m = re.exec(s);
  }
}
run("ab=42;cd= -17;ef=0x1F x=+9;=7;zz=");
run("日本a=3;x=　 0x10;😀b=12345678901234567890;c= 7;d=７");

let total = 0;
const re = /([a-z]+)=(\d+)/g;
for (let i = 0; i < 3; i = i + 1) {
  re.lastIndex = 0;
  let m = re.exec("k=1;kk=22;kkk=333");
  while (m !== null) {
    total = total + m.index + m[1].length + parseInt(m[2], 10);
    m = re.exec("k=1;kk=22;kkk=333");
  }
}
console.log(total);
