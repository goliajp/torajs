// Date.UTC arity 1-7 per ES §21.4.2.21. Trailing args default to
// month=0, day=1, rest=0.

console.log(Date.UTC(2024, 0, 1, 0, 0, 0, 0))  // 7-arg (existing path)
console.log(Date.UTC(2024, 0, 1, 0, 0, 0))     // ms=0 defaulted
console.log(Date.UTC(2024, 0, 1, 0, 0))        // sec=0 defaulted
console.log(Date.UTC(2024, 0, 1, 0))           // min=0 defaulted
console.log(Date.UTC(2024, 0, 1))              // hour=0 defaulted
console.log(Date.UTC(2024, 5))                 // day=1, hour=min=sec=ms=0
console.log(Date.UTC(2024))                    // month=0, day=1, rest=0

// boundary: epoch zero year (month=0 day=1)
console.log(Date.UTC(1970))

// negative year (BCE-ish)
console.log(Date.UTC(0, 0, 1))
