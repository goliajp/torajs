// A constant index past the pattern's capture count is an
// out-of-range element read: `undefined`, and `.length` on it throws.

function run(s: string): void {
  const re = /(x)(y)/;
  let m = re.exec(s);
  if (m !== null) {
    console.log(m[0], m[1], m[2], m[3], m[7], typeof m[7], m[7] === undefined);
    try {
      console.log(m[3].length);
    } catch (e) {
      console.log("caught", e instanceof TypeError);
    }
    console.log(m[0].length + m[2].length);
  } else {
    console.log("miss");
  }
}
run("zzxyzz");
run("zz");
