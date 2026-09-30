// the second conjunct has a side effect: it runs exactly when the
// first did not decide, and the body sees only what both decided
let probes = 0;
function above(d: number): boolean {
  probes = probes + 1;
  return d > 57;
}
function parseDigits(src: string): number {
  let i = 0;
  let v = 0;
  while (i < src.length) {
    const d = src.charCodeAt(i);
    if (d < 48 || above(d)) break;
    v = v * 10 + (d - 48);
    i = i + 1;
  }
  return v;
}
console.log(parseDigits("123"), probes);
console.log(parseDigits("12z3"), probes);
console.log(parseDigits(" 1"), probes);
console.log(parseDigits("99999999999999999999"), probes);

// the opaque side first: the body learns only the second compare
let seen = 0;
function stop(): boolean {
  seen = seen + 1;
  return seen > 4;
}
function parseSome(src: string): number {
  let i = 0;
  let v = 0;
  while (i < src.length) {
    const d = src.charCodeAt(i);
    if (stop() || d < 48 || d > 57) break;
    v = v * 10 + (d - 48);
    i = i + 1;
  }
  return v;
}
console.log(parseSome("123456789"), seen);
seen = 0;
console.log(parseSome("12x456789"), seen);

// the second conjunct writes the value the first compared: the body
// reads the written value, which the compares said nothing about
let d = 0;
function bump(): boolean {
  d = d + 1;
  return d > 60;
}
function walk(src: string): number {
  let i = 0;
  let acc = 0;
  while (i < src.length) {
    d = src.charCodeAt(i);
    if (d < 48 || bump()) break;
    if (d > 57) acc = acc + 1000000;
    acc = acc * 100 + (d - 48);
    i = i + 1;
  }
  return acc;
}
console.log(walk("129"), d);
console.log(walk("9"), d);
console.log(walk(":"), d);
console.log(walk("<5"), d);
