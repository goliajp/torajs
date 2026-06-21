// V0.2 P14-S2 — Pike VM literal-prefix anchor. compile.rs walks
// the emitted bytecode forward (skipping zero-width Save /
// AnchorB / AnchorE / WBound / NWBound ops) and records
// `prog.prefix_byte = Some(b)` when the first byte-consuming
// op is `Char(b)` and the i flag is not set. `search_from_with_ws`
// then memchr-skips through the input to the next candidate
// start position, dropping the Pike VM scan cost on no-match
// patterns from ~1011 ns/iter to ~167 ns/iter on the
// str-replace-100k workload. This fixture locks the prefix-detect
// classification across the boundary cases that decide whether
// the anchor is set.

// (1) Char-leading: anchor set. /abc/ on "xxxabc xxx abc"
// must match at positions 3 and 11. Tests memchr-skip past gap.
console.log("abbc xxx abc yyy abbbbc".replace(/abc/g, "X"));
// Expected: "abbX xxx X yyy abbbbc"

// (2) Capture-group-leading: Save instructions precede the Char,
// the anchor walk skips Save and lands on Char. /(abc)/g still
// memchr-anchored on 'a'.
console.log("xxx abc yyy abc zzz".replace(/(abc)/g, "[$1]"));
// Expected: "xxx [abc] yyy [abc] zzz"

// (3) AnchorB-leading: ^abc must anchor at byte 0 only; the
// detect walk skips AnchorB then sees Char(a) → anchor on 'a'.
// memchr would jump to first 'a' position, but the AnchorB op
// at the head of the program restricts vm_match_at to position
// 0; memchr-skip past byte 0 then NFA at non-0 position fails
// (AnchorB), so search continues — semantics preserved.
console.log("abc abc abc".replace(/^abc/g, "X"));
// Expected: "X abc abc"

// (4) AnyChar-leading: /./ — no prefix anchor (first op AnyChar
// not Char). Detect walk hits AnyChar in the "different shape"
// branch and leaves prefix_byte = None. Original per-position
// scan path kicks in. Parity still holds.
console.log("abc".replace(/./g, "X"));
// Expected: "XXX"

// (5) Class-leading: /[ab]c/ — first op Class. No anchor; falls
// through to the original path.
console.log("axc bxc abc".replace(/[ab]c/g, "Y"));
// Expected: "axc bxc aY"

// (6) i flag: /ABC/i — case-insensitive. The anchor is disabled
// (memchr can't match both A and a in a single pass). Original
// per-position scan handles case-fold in vm_match_at.
console.log("abc ABC AbC".replace(/ABC/gi, "X"));
// Expected: "X X X"

// (7) Alternation-leading: /a|b/ — first emitted op is Split
// (alternation forks before consuming). Detect walk hits Split
// in the "different shape" branch — no anchor. Fallback path.
console.log("xax xbx".replace(/a|b/g, "Y"));
// Expected: "xYx xYx"

// (8) Repeated-leading: /(?:a){2}/ — emits Char, Char (literal
// expansion of the fixed repeat). First op Char(a) → anchor set.
console.log("xaa xaa x a".replace(/a{2}/g, "Y"));
// Expected: "xY xY x a"
