// P-iter Phase 3 fallback verification — every "must NOT rewrite"
// pattern below has to keep producing bun-equivalent output via the
// untouched eager-Array<Substr> path. If the rewrite triggers
// incorrectly on any of these, behavior diverges from bun.

// (1) Body uses arr[i+1] — random access, must NOT rewrite.
function fallback_jindex(s: string): number {
  let parts: string[] = s.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    let cur: string = parts[i];
    total = total + cur.length;
    if (i + 1 < parts.length) {
      let next: string = parts[i + 1];  // arr[i+1] ← disqualifies
      total = total + next.length;
    }
  }
  return total;
}
console.log(fallback_jindex("ab,cd,ef"));

// (2) parts used after the loop — must NOT rewrite (X needs to live).
function fallback_use_after(s: string): number {
  let parts: string[] = s.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    total = total + parts[i].length;
  }
  total = total + parts.length;  // X.length after loop ← disqualifies
  return total;
}
console.log(fallback_use_after("ab,cd,ef"));

// (3) Body has X.length inside — must NOT rewrite.
function fallback_inner_length(s: string): number {
  let parts: string[] = s.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    total = total + parts[i].length + parts.length;
  }
  return total;
}
console.log(fallback_inner_length("ab,cd,ef"));

// (4) Happy path — should rewrite + still match bun.
function happy_path(s: string): number {
  let parts: string[] = s.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    let item: string = parts[i];
    total = total + item.length;
  }
  return total;
}
console.log(happy_path("ab,cd,ef"));

// (5) Counter `i` used in body — rewrite preserves manual counter.
function happy_with_i(s: string): number {
  let parts: string[] = s.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    total = total + parts[i].length + i;
  }
  return total;
}
console.log(happy_with_i("ab,cd,ef"));
