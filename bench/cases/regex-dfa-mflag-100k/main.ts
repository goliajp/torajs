// Multiline `^` DFA fast-path (chunk 8.8 RE_FLAG_M) — 100k iter.
// `/^line/m` widens `^` to "byte 0 or after `\n`". chunk 8.8 folds
// `mflag` into PositionCtx so the DFA's start_mid state honours
// line-internal anchors; the wire selects entry state per `\n`.
const re = /^line/m
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'pre\nline-text\nend ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
