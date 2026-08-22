// A `\p{...}` entry state has no 256-way transition row — the class
// replaced it — so the scan that skips hopeless start positions used
// to give up and let every position run the full anchored matcher.
// It can ask the class's own ASCII bitmap instead, and these are the
// places where asking it wrongly would show.
const probes: string[] = [
  // the plain skip: leading non-members, then the match
  JSON.stringify("   hello world".match(/\p{L}+/u)),
  JSON.stringify("123 abc".match(/\p{L}+/u)),
  JSON.stringify("!!!".match(/\p{L}+/u)),
  JSON.stringify("".match(/\p{L}+/u)),
  JSON.stringify("a".match(/\p{L}+/u)),
  // the first viable position must be exact, not merely early
  JSON.stringify("  42x".match(/\p{L}+/u)),
  JSON.stringify("  42x".search(/\p{L}+/u)),
  JSON.stringify("xy  ".search(/\p{L}+/u)),
  // a byte at or above 0x80 cannot be decided from the bitmap and has
  // to be admitted — these all start non-ASCII
  JSON.stringify("   日本語".match(/\p{L}+/u)),
  JSON.stringify("日本語".match(/\p{L}+/u)),
  JSON.stringify("   é".match(/\p{L}+/u)),
  JSON.stringify("  ÀÉÎ  ".match(/\p{L}+/u)),
  // negated property: the members are what gets skipped now
  JSON.stringify("abc123".match(/\P{L}+/u)),
  JSON.stringify("abc   ".match(/\P{L}+/u)),
  JSON.stringify("日本1".match(/\P{L}+/u)),
  // other property classes
  JSON.stringify("abc42def".match(/\p{N}+/u)),
  JSON.stringify("  \t x".match(/\p{White_Space}+/u)),
  JSON.stringify("aBc".match(/\p{Lu}+/u)),
  // a class miss that is NOT fatal — the alternation keeps the
  // position alive, so nothing may be skipped
  JSON.stringify("...abc".match(/\p{L}+|\.+/u)),
  JSON.stringify("123abc".match(/\p{L}+|\d+/u)),
  // global, so the scan restarts mid-string many times
  JSON.stringify("a1b22c333d".match(/\p{L}+/gu)),
  JSON.stringify("  x  y  z  ".match(/\p{L}+/gu)),
  JSON.stringify("日1本2語".match(/\p{L}+/gu)),
  // anchored and word-boundary shapes take the other entry path
  JSON.stringify("  abc".match(/^\p{L}+/u)),
  JSON.stringify("abc".match(/^\p{L}+/u)),
  JSON.stringify("x abc y".match(/\b\p{L}\p{L}\p{L}\b/u)),
];
for (const p of probes) console.log(p);
