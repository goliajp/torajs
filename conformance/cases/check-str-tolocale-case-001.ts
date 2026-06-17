// `s.toLocaleLowerCase()` / `s.toLocaleUpperCase()` per ES §22.1.3.21
// / §22.1.3.23 — locale-aware case conversion. tr's subset is en-US
// only (matching bun / Node on macOS default locale), so the locale
// arg is accepted and ignored; routing to the non-locale form
// reproduces bun's output bit-for-bit.
console.log('Hello'.toLocaleLowerCase())
console.log('Hello'.toLocaleUpperCase())
console.log('AbCdEf'.toLocaleLowerCase())
console.log('AbCdEf'.toLocaleUpperCase())
console.log(''.toLocaleLowerCase())
console.log(''.toLocaleUpperCase())

// Locale arg accepted (ignored).
console.log('Hello'.toLocaleLowerCase('en-US'))
console.log('Hello'.toLocaleUpperCase('en-US'))

// Round-trip parity with the non-locale form.
const s = 'MixedCase'
console.log(s.toLocaleLowerCase() === s.toLowerCase())
console.log(s.toLocaleUpperCase() === s.toUpperCase())

// In chain.
console.log('  HELLO  '.trim().toLocaleLowerCase())
