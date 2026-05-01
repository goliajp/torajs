// Adapted from test262: language/expressions/arrow-function/* — a closure
// captures an outer-scope array and pushes into it across many
// invocations. Each call must see the prior call's pushes. Tr previously
// crashed at SIGABRT because the closure body's local cap_slot updated
// after `arr_push` realloc, but the env block kept the pre-realloc
// pointer — the next invocation re-loaded the stale ptr from env.
// ssa-lower now mirrors every captured-array push back to env+offset so
// the env stays in sync across calls.
//
// To stay bun-portable, the array is observed only via the closure (each
// invocation returns its current length). Outer-scope reads of the
// captured array under tr's value-shape capture remain a documented
// limitation; this test exercises the closure-internal consistency the
// env-writeback fix specifically delivers.
function check(): number {
  let xs: number[] = [];
  let push_and_count = (k: number): number => {
    xs.push(k);
    xs.push(k * 10);
    xs.push(k * 100);
    return xs.length;
  };

  let n1 = push_and_count(1);
  if (n1 !== 3) { throw "#1"; }
  let n2 = push_and_count(2);
  if (n2 !== 6) { throw "#2: 2nd invocation should see prior push"; }
  let n3 = push_and_count(3);
  if (n3 !== 9) { throw "#3"; }

  // Force many reallocs — env writeback is hit on every push.
  for (let i: number = 0; i < 50; i = i + 1) {
    push_and_count(i);
  }
  let final_len = push_and_count(999);
  if (final_len !== 162) { throw "#4: final len after 50+1 mass-pushes"; }
  return 0;
}
console.log(check());
