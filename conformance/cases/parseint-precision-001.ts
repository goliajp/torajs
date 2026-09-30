// parseInt rounds the exact integer once (§19.2.5 step 14: radix 10 up
// to 20 significant digits and the power-of-two radices are exact);
// accumulating in a double rounds once per digit and drifts past 2^53.
// Acceptance: byte-equal with bun.

const cases: [string, number][] = [
  ["9007199254740993", 10],
  ["9007199254740995", 10],
  ["18014398509481987", 10],
  ["-9007199254740993", 10],
  ["12345678901234567890", 10],
  ["0x20000000000003", 16],
  ["0x200000000000011", 16],
  ["ffffffffffffffffffff", 16],
  ["100000000000000000000000000000000000000000000000000011", 2],
  ["777777777777777777777", 8],
  ["00000000000000000000042", 10],
  ["9007199254740992", 10],
];
for (const [s, r] of cases) {
  console.log(s, r, parseInt(s, r));
}
console.log(parseInt("1".repeat(1100), 2));
console.log(Number.parseInt("99999999999999999999"));
