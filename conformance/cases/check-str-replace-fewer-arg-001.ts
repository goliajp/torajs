// S207 — String.replace / replaceAll accept fewer-than-2 args per
// ES §22.1.3.18 / §22.1.3.19. Missing slots default to undefined →
// ToString → "undefined", so the runtime helper sees a valid Str
// needle / replacement. Bun-aligned across all shapes below.

// 0-arg: needle = "undefined", repl = "undefined". No match in
// "hello" → identity.
console.log("hello".replace())
console.log("hello".replaceAll())

// 0-arg with haystack containing literal "undefined": both needle
// and replacement are "undefined", net result still "xundefinedy".
console.log("xundefinedy".replace())
console.log("xundefinedy".replaceAll())

// 1-arg: needle taken from arg, repl defaults to "undefined".
console.log("hello".replace("l"))
console.log("hello".replaceAll("l"))

// 1-arg with explicit-undefined needle equivalent to 0-arg.
console.log("hello".replace(undefined))
console.log("hello".replaceAll(undefined))

// 2-arg with explicit-undefined repl matches the implicit form.
console.log("hello".replace("l", undefined))
console.log("hello".replaceAll("l", undefined))

// Confirm existing 2-arg full-undefined path unchanged.
console.log("hello".replace(undefined, undefined))
console.log("hello".replaceAll(undefined, undefined))
