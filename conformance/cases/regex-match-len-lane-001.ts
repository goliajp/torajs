// a match result only null-tested or asked m[0].length is held as the
// first match's length; every other use keeps the array. both faces
// must answer exactly as the array would.
const words = /[a-z]+/;
const empty = /x*/;
const glob = /o/g;
const stick = /b/y;

function scalarFace(s: string): number {
  let total = 0;
  const m = s.match(words);
  if (m !== null) total = total + m[0].length;
  const e = s.match(empty);
  if (e) total = total + 100 + e[0].length;
  const n = s.match(/zz+/);
  total = total + (n === null ? 1000 : 0) + (!n ? 10000 : 0);
  const g = s.match(glob);
  total = total + (g != null ? g[0].length * 7 : 0);
  return total;
}
console.log(scalarFace("hello world"), scalarFace("123"), scalarFace(""));
console.log(glob.lastIndex);

function stickyFace(): string {
  stick.lastIndex = 1;
  const a = "abc".match(stick);
  const hit = a !== null ? a[0].length : -1;
  const at = stick.lastIndex;
  const b = "abc".match(stick);
  return hit + ":" + at + ":" + (b === null ? "null" : "x") + ":" + stick.lastIndex;
}
console.log(stickyFace());

function unicodeFace(): string {
  let out = "";
  for (const s of ["é😀a", "\uD800x", "ab😀😀c"]) {
    const m = s.match(/[^a]+/);
    const u = s.match(/[^a]+/u);
    out = out + (m !== null ? m[0].length : -1) + "," + (u !== null ? u[0].length : -1) + ";";
  }
  return out;
}
console.log(unicodeFace());

function loopFace(): number {
  let total = 0;
  for (let i = 0; i < 50; i++) {
    const s = "item " + i.toString();
    const m = s.match(/\d+/);
    if (m !== null) total = total + m[0].length;
    const k = s.match(/q/);
    while (k) {
      total = total + 1;
      break;
    }
  }
  return total;
}
console.log(loopFace());

function arrayFace(s: string): string {
  const a = s.match(/(\w)(\w)/);
  const first = a !== null ? a[1] : "-";
  const b = s.match(/\w+/g);
  const count = b !== null ? b.length : 0;
  const c = s.match(/o/);
  const str = c !== null ? c[0] + "!" : "";
  const d = s.match(/l+/);
  const kept = d;
  const e = s.match(/w/);
  const idx = e !== null ? e.index : -1;
  return first + count + str + (kept === null ? "n" : kept[0]) + idx;
}
console.log(arrayFace("hello world"), arrayFace("x"));

function shadowFace(s: string): number {
  const m = s.match(/l+/);
  let r = 0;
  if (m !== null) {
    const m = [9, 8];
    r = m[0];
  }
  return r + (m !== null ? m[0].length : 0);
}
console.log(shadowFace("hello"));
