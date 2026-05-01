// Integration: grid of stdlib usage — each section exercises a
// distinct corner of the runtime to catch interaction regressions.
function check(): number {
  // Math grid.
  if (Math.abs(-5) !== 5) { throw "#1"; }
  if (Math.sqrt(16) !== 4) { throw "#2"; }
  if (Math.pow(3, 4) !== 81) { throw "#3"; }
  if (Math.floor(2.7) !== 2) { throw "#4"; }
  if (Math.ceil(2.1) !== 3) { throw "#5"; }
  if (Math.sign(-7) !== -1) { throw "#6"; }
  if (Math.sign(0) !== 0) { throw "#7"; }
  if (Math.sign(42) !== 1) { throw "#8"; }
  if (Math.imul(3, 4) !== 12) { throw "#9"; }
  if (Math.clz32(1) !== 31) { throw "#10"; }

  // Array grid.
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  if (xs.indexOf(5) !== 4) { throw "#11"; }
  if (xs.lastIndexOf(1) !== 3) { throw "#12"; }
  if (xs.includes(9) !== true) { throw "#13"; }
  if (xs.includes(99) !== false) { throw "#14"; }
  if (xs.length !== 8) { throw "#15"; }
  let dst = xs.slice(2, 5);
  if (dst.length !== 3) { throw "#16"; }
  if (dst[0] !== 4) { throw "#17"; }
  let cat = xs.concat([100, 200]);
  if (cat.length !== 10) { throw "#18"; }
  if (cat[9] !== 200) { throw "#19"; }

  // String grid.
  let s = "Hello, World!";
  if (s.length !== 13) { throw "#20"; }
  if (s.toUpperCase() !== "HELLO, WORLD!") { throw "#21"; }
  if (s.toLowerCase() !== "hello, world!") { throw "#22"; }
  if (s.indexOf("World") !== 7) { throw "#23"; }
  if (s.includes("foo") !== false) { throw "#24"; }
  if (s.replace("World", "earth") !== "Hello, earth!") { throw "#25"; }
  if (s.replaceAll("l", "L") !== "HeLLo, WorLd!") { throw "#26"; }
  if (s.slice(7, 12) !== "World") { throw "#27"; }
  if (s.startsWith("Hello") !== true) { throw "#28"; }
  if (s.endsWith("!") !== true) { throw "#29"; }
  if ("  trim me  ".trim() !== "trim me") { throw "#30"; }

  // Number grid.
  if (Number.isInteger(7) !== true) { throw "#31"; }
  if (Number.isInteger(7.5) !== false) { throw "#32"; }
  if (Number.isFinite(0) !== true) { throw "#33"; }
  if (Number.isNaN(0) !== false) { throw "#34"; }
  if (Number.isSafeInteger(Number.MAX_SAFE_INTEGER) !== true) { throw "#35"; }
  if ((42).toString() !== "42") { throw "#36"; }
  if ((3.14).toFixed(1) !== "3.1") { throw "#37"; }

  // Coercion grid.
  if (String(123) !== "123") { throw "#38"; }
  if (Number("456") !== 456) { throw "#39"; }
  if (parseInt("ff", 16) !== 255) { throw "#40"; }
  return 0;
}
console.log(check());
