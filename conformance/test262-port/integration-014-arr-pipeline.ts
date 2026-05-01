// Integration: pipeline of array transforms — map, filter, reduce
// chained into a single result. Exercises the M6.2 closure-arg
// machinery (predicate/comparator/accumulator) over a single array.
function check(): number {
  let nums: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

  // Sum of squares of even numbers.
  let r1 = nums
    .filter((n: number): boolean => n % 2 === 0)
    .map((n: number): number => n * n)
    .reduce((acc: number, x: number): number => acc + x, 0);
  // 2² + 4² + 6² + 8² + 10² = 4 + 16 + 36 + 64 + 100 = 220
  if (r1 !== 220) { throw "#1"; }

  // Count elements > 5.
  let r2 = nums.filter((n: number): boolean => n > 5).length;
  if (r2 !== 5) { throw "#2"; }

  // Max via reduce.
  let r3 = nums.reduce(
    (acc: number, x: number): number => x > acc ? x : acc,
    nums[0]
  );
  if (r3 !== 10) { throw "#3"; }

  // Sum via reduce.
  let total = nums.reduce((a: number, x: number): number => a + x, 0);
  if (total !== 55) { throw "#4: 1+2+...+10"; }

  // Some / every / findIndex.
  if (nums.some((n: number): boolean => n > 100) !== false) { throw "#5"; }
  if (nums.every((n: number): boolean => n > 0) !== true) { throw "#6"; }
  if (nums.findIndex((n: number): boolean => n === 7) !== 6) { throw "#7"; }

  // String[] pipeline — case fold + join.
  let labels: string[] = ["alpha", "BETA", "Gamma"];
  let r5 = labels.map((s: string): string => s.toLowerCase()).join("-");
  if (r5 !== "alpha-beta-gamma") { throw "#8"; }
  return 0;
}
console.log(check());
