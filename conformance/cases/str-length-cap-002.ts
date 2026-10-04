// A string method or join whose result would pass the longest string
// throws the RangeError at the call itself: nothing after it in the
// same function runs. Each case is rejected before anything near that
// size is allocated.

function pad(): string {
  const s = "a".padStart(2 ** 31);
  console.log("after pad");
  return s;
}

function padEnd(): string {
  const s = "a".padEnd(2 ** 31, "xy");
  console.log("after padEnd");
  return s;
}

function replaceAll(k: string, y: string): string {
  const s = k.replaceAll("x", y);
  console.log("after replaceAll");
  return s;
}

function join(parts: string[]): string {
  const s = parts.join("");
  console.log("after join");
  return s;
}

function toStr(parts: string[]): string {
  const s = parts.toString();
  console.log("after toString");
  return s;
}

function repeat(): string {
  const s = "ab".repeat(2 ** 30);
  console.log("after repeat");
  return s;
}

const k64 = "x".repeat(1 << 16);
const y64 = "y".repeat(1 << 16);
const mb = "z".repeat(1 << 20);
const parts: string[] = [];
for (let i = 0; i < 2048; i++) parts.push(mb);

try { pad(); } catch (e) { console.log("pad", e instanceof RangeError); }
try { padEnd(); } catch (e) { console.log("padEnd", e instanceof RangeError); }
try { replaceAll(k64, y64); } catch (e) { console.log("replaceAll", e instanceof RangeError); }
try { join(parts); } catch (e) { console.log("join", e instanceof RangeError); }
try { toStr(parts); } catch (e) { console.log("toString", e instanceof RangeError); }
try { repeat(); } catch (e) { console.log("repeat", e instanceof RangeError); }
console.log("done", replaceAll("axb", "y"), join(["p", "q"]));
