// v0.3 #3: process surface (minimum) — platform / cwd / exit.
// `process.exit` is intentionally not exercised here (would terminate
// the test before assertions run); covered by hand-written cases.

console.log(process.platform);
console.log(process.cwd().length > 0);
console.log(process.cwd().startsWith("/") || process.cwd().substring(1, 2) === ":");
