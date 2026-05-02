// Adapted from test262: built-ins/Array/prototype/find +
// Array/prototype/findLast. tr's subset returns the element type
// itself instead of `T | undefined`; tests here only cover the
// must-be-found path so both bun and tr agree on the result.
function check(): number {
  // Number find — first match.
  let xs: number[] = [10, 20, 30, 40];
  let v: number = xs.find((n: number): boolean => n > 25);
  if (v !== 30) { throw "#1: v"; }

  // Number findLast — last match (reverse scan).
  let last: number = xs.findLast((n: number): boolean => n < 25);
  if (last !== 20) { throw "#2: last"; }

  // String find — refcounted path. Result is owned (rc_inc'd at hit
  // site so the returned binding can drop independently).
  let ss: string[] = ["alpha", "beta", "gamma"];
  let s: string = ss.find((s: string): boolean => s.length > 4);
  if (s !== "alpha") { throw "#3: s"; }

  let sl: string = ss.findLast((s: string): boolean => s.length === 4);
  if (sl !== "beta") { throw "#4: sl"; }

  // Always-true predicate hits first / last element.
  let first: number = xs.find((n: number): boolean => true);
  if (first !== 10) { throw "#5: first"; }
  let lastOne: number = xs.findLast((n: number): boolean => true);
  if (lastOne !== 40) { throw "#6: lastOne"; }
  return 0;
}
console.log(check());
