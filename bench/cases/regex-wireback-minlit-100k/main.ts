// Round 4 wire-back substrate isolation — 100k iterations.
// Trivial pattern `/x/` against trivial haystack `"x"` so the DFA
// inner-loop cost is minimal (1 byte to a single-state accept),
// leaving per-iter cost dominated by wire-back:
// - __torajs_arr_alloc(0) — fresh empty Array per match
// - str_from_bytes(&s[0..1]) → __torajs_str_alloc_pooled — Str(`"x"`)
// - __torajs_arr_push — push group 0
// - attach_exec_props — set .index / .input via dynobj_set x2
// Wire-back path is `match_op.rs::__torajs_str_match_regex`
// (non-global branch falls through `attach_exec_props` + `attach_groups`).
const re = /x/
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const m = 'x'.match(re)
  if (m !== null) total = total + 1
}
console.log(total)
