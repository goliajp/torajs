// P11.5-A3 — Final_Sigma context-dependent mapping per UAX #21.
// `Σ (U+03A3)` in lowercase fold becomes `ς (U+03C2)` when preceded
// by a Cased letter and not followed by a Cased letter; otherwise
// it stays the default `σ (U+03C3)`.  Pre-A3 every Σ went through
// the Simple mapping to σ, so `"ΟΔΥΣΣΕΥΣ".toLowerCase()` produced
// `"οδυσσευσ"` instead of bun's `"οδυσσευς"`.
//
// Simplification: tora's A3 check looks at immediate adjacent
// source code points.  The full UAX #21 rule allows skipping
// Case_Ignorable on both sides; cases involving combining marks or
// punctuation between Σ and the Cased neighbor lower to the
// non-final form here (P11.5-A4 follow-up).
//
// Coverage:
// - Word-final Σ in real Greek words
// - Word-initial Σ (no preceding Cased -> σ)
// - Bare Σ (no neighbor -> σ)
// - Σ in the middle of Cased letters (both neighbors Cased -> σ)
// - Consecutive Σ (first stays σ, last becomes ς)
// - toUpperCase regression (Σ stays Σ — Final_Sigma is lower-only)

console.log('--- Greek words ---')
console.log('ΟΔΥΣΣΕΥΣ'.toLowerCase())
console.log('ΣΟΦΟΣ'.toLowerCase())
console.log('ΑΘΗΝΑ'.toLowerCase())
console.log('ΚΟΣΜΟΣ'.toLowerCase())

console.log('--- Σ context isolation ---')
console.log('Σ'.toLowerCase())
console.log('AΣ'.toLowerCase())
console.log('ΣA'.toLowerCase())
console.log('AΣA'.toLowerCase())

console.log('--- consecutive Σ ---')
console.log('ΣΣ'.toLowerCase())
console.log('ΟΣΟΣ'.toLowerCase())
console.log('ΣΣΣ'.toLowerCase())

console.log('--- toUpperCase regression (Σ is upper-only) ---')
console.log('σοφος'.toUpperCase())
console.log('οδυσσευς'.toUpperCase())

console.log('--- Mixed with previous A2 fixtures ---')
console.log('Καλημέρα ΚΟΣΜΕ'.toLowerCase())
console.log('Ηλιος'.toLowerCase())
