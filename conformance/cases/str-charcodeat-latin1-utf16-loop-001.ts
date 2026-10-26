// One `charCodeAt` call site inside a loop reads both encodings: a
// Latin-1 string (one byte per code unit) and a UTF-16 string holding
// U+4E2D (one halfword per code unit). The payload address is the
// same for every read of one receiver, so the loop may compute it
// once; each read must still pick the receiver's own encoding.
function scan(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    out = out + s.charCodeAt(i) + " ";
  }
  return out;
}

// the unproven lane: the counter runs one past the end on purpose
function scanPast(s: string): string {
  let out = "";
  let i = 0;
  while (i <= s.length) {
    out = out + s.charCodeAt(i) + " ";
    i = i + 1;
  }
  return out;
}

function sumAll(xs: string[]): number {
  let total = 0;
  for (const s of xs) {
    for (let i = 0; i < s.length; i++) total = total + s.charCodeAt(i);
  }
  return total;
}

const lat = "az09 ~éÿ";
const wide = "a中bé";
console.log(scan(lat));
console.log(scan(wide));
console.log(scanPast(lat));
console.log(scanPast(wide));
console.log(sumAll([lat, wide, "", "中", "x"]));

// out of range on either side is NaN
console.log(lat.charCodeAt(-1), lat.charCodeAt(lat.length), wide.charCodeAt(99));

// no argument reads index 0; constant indices keep reading the payload
console.log(lat.charCodeAt(), wide.charCodeAt(), lat.charCodeAt(0), wide.charCodeAt(1));
const empty = "";
console.log(empty.charCodeAt(), empty.charCodeAt(0));

// a run-time index outside any loop
const at = process.argv.length > 99 ? 0 : 2;
console.log(lat.charCodeAt(at), wide.charCodeAt(at - 1));
