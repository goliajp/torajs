// Integration: comprehensive stdlib coverage in a single program.
// Each section exercises a different surface: 30+ assertions across
// Math, Number, String, Array, Object, JSON, console, parsing, and
// coercion paths. Catches regressions when any change perturbs a
// shared code path.
function check(): number {
  // Math grid (full surface).
  if (Math.abs(-5) !== 5) { throw "#1"; }
  if (Math.sqrt(16) !== 4) { throw "#2"; }
  if (Math.cbrt(27) !== 3) { throw "#3"; }
  if (Math.pow(2, 10) !== 1024) { throw "#4"; }
  if (Math.imul(-3, 4) !== -12) { throw "#5"; }
  if (Math.sign(-0.5) !== -1) { throw "#6"; }
  if (Math.sign(0) !== 0) { throw "#7"; }
  let r = Math.random();
  if (r < 0 || r >= 1) { throw "#8: random range"; }

  // Variadic Math.
  if (Math.min(5, 3, 8, 1, 9) !== 1) { throw "#9"; }
  if (Math.max(5, 3, 8, 1, 9) !== 9) { throw "#10"; }
  if (Math.hypot(3, 4, 12) !== 13) { throw "#11"; }

  // Number predicates.
  if (Number.isInteger(7) !== true) { throw "#12"; }
  if (Number.isInteger(7.5) !== false) { throw "#13"; }
  if (Number.isFinite(0) !== true) { throw "#14"; }
  if (Number.isSafeInteger(1) !== true) { throw "#15"; }

  // String pipeline.
  let s = "  Hello World  ";
  if (s.trim().toLowerCase() !== "hello world") { throw "#16"; }
  if ("abc".repeat(3) !== "abcabcabc") { throw "#17"; }
  if ("hello".replace("l", "L") !== "heLlo") { throw "#18"; }
  if ("hello".replaceAll("l", "L") !== "heLLo") { throw "#19"; }
  if ("hello".at(-1) !== "o") { throw "#20"; }
  if ("hello".charCodeAt(0) !== 104) { throw "#21"; }
  if (String.fromCharCode(104) !== "h") { throw "#22"; }
  if ("abc".padStart(5, "_") !== "__abc") { throw "#23"; }
  if ("abc".padEnd(5, "_") !== "abc__") { throw "#24"; }
  if ("abc".localeCompare("abd") !== -1) { throw "#25"; }

  // Array pipeline.
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  if (xs.includes(5) !== true) { throw "#26"; }
  if (xs.lastIndexOf(1) !== 3) { throw "#27"; }
  if (xs.findIndex((n: number): boolean => n > 5) !== 5) { throw "#28"; }
  if (xs.some((n: number): boolean => n === 9) !== true) { throw "#29"; }
  if (xs.every((n: number): boolean => n > 0) !== true) { throw "#30"; }
  let sum = xs.reduce((a: number, b: number): number => a + b, 0);
  if (sum !== 31) { throw "#31"; }

  // Coercion.
  if (String(42) !== "42") { throw "#32"; }
  if (Number("42") !== 42) { throw "#33"; }
  if (parseInt("ff", 16) !== 255) { throw "#34"; }
  if (parseFloat("3.14") !== 3.14) { throw "#35"; }

  // Array.isArray.
  if (Array.isArray(xs) !== true) { throw "#36"; }
  if (Array.isArray(s) !== false) { throw "#37"; }

  // JSON.
  if (JSON.stringify(42) !== "42") { throw "#38"; }
  if (JSON.stringify("hi") !== "\"hi\"") { throw "#39"; }
  if (JSON.stringify([1, 2]) !== "[1,2]") { throw "#40"; }
  return 0;
}
console.log(check());
