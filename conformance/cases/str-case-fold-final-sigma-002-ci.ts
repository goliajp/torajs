// P11.5-A4 — Final_Sigma Case_Ignorable skip rule per UAX #21.
// Pre-A4 the Final_Sigma check only looked at immediate adjacent
// source code points, so non-significant separators (period,
// combining marks, etc.) between Σ and the surrounding Cased
// letters blocked the rule.  Bun honors the full skip rule via V8
// ICU; tora now matches by walking past Case_Ignorable cps when
// looking back / forward.
//
// Coverage:
// - Period (Po, Case_Ignorable) before / after Σ
// - Combining mark (Mn, Case_Ignorable) before Σ
// - Space (Zs, NOT Case_Ignorable) blocks the skip
// - Multiple CI cps stacked
// - A3 fixtures should regress-unchanged

console.log('--- period CI before Σ ---')
console.log('A.Σ'.toLowerCase())
console.log('A.Σ.A'.toLowerCase())
console.log('AB.Σ'.toLowerCase())

console.log('--- combining mark CI before Σ ---')
console.log('ÁΣ'.toLowerCase())
console.log('ÉΣ'.toLowerCase())

console.log('--- combining mark CI after Σ (still no Cased after) ---')
console.log('Σ́'.toLowerCase())
console.log('AΣ́'.toLowerCase())

console.log('--- space (Zs) blocks ---')
console.log('A Σ'.toLowerCase())
console.log('A  Σ'.toLowerCase())

console.log('--- multiple CI stacked ---')
console.log('A..Σ'.toLowerCase())
console.log('A.,Σ'.toLowerCase())

console.log('--- A3 regression (no CI between, still works) ---')
console.log('ΟΔΥΣΣΕΥΣ'.toLowerCase())
console.log('Σ'.toLowerCase())
console.log('ΣΣ'.toLowerCase())
console.log('AΣ'.toLowerCase())
console.log('AΣA'.toLowerCase())
