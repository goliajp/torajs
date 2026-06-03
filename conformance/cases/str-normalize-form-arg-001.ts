// P11.6-S5 — String.prototype.normalize form argument handling per
// ES §22.1.3.13 steps 2-3:
//   step 2: form defaults to "NFC" when undefined
//   step 3: form must be one of "NFC" / "NFD" / "NFKC" / "NFKD",
//           else throw RangeError
//
// Coverage:
// - normalize() (no arg) === normalize("NFC")
// - All four valid forms accepted
// - Invalid form throws a catchable RangeError
//
// Note: e.message is intentionally NOT asserted here. The current
// torajs throw infrastructure raises RangeError as a thrown value
// but does not yet populate e.message — same shape used by
// P11.1-S6 fromCodePoint and tracked as a separate trunk follow-up.

console.log('--- default form === NFC ---')
const dec = 'é'
console.log(dec.normalize() === dec.normalize('NFC'))
console.log(dec.normalize() === 'é')
console.log('café'.normalize() === 'café')

console.log('--- four valid forms accepted ---')
const s = 'café'
console.log(s.normalize('NFC').length)
console.log(s.normalize('NFD').length)
console.log(s.normalize('NFKC').length)
console.log(s.normalize('NFKD').length)

console.log('--- invalid form throws ---')
try {
  'abc'.normalize('XYZ')
  console.log('NO_THROW_BAD')
} catch (_e) {
  console.log('CAUGHT_BAD')
}

try {
  'abc'.normalize('nfc') // lowercase is invalid per spec
  console.log('NO_THROW_LOWER')
} catch (_e) {
  console.log('CAUGHT_LOWER')
}

try {
  'abc'.normalize('')
  console.log('NO_THROW_EMPTY')
} catch (_e) {
  console.log('CAUGHT_EMPTY')
}

try {
  'abc'.normalize('NFCC')
  console.log('NO_THROW_LONG')
} catch (_e) {
  console.log('CAUGHT_LONG')
}
