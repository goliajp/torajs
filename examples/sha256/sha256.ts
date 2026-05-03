// SHA-256 reference implementation, single-file, no dependencies.
//
// Tests the torajs runtime against an algorithm-heavy real-world TS
// program: bit manipulation, fixed-size arrays of integers, generic
// helpers, message-digest framing. Output is the lowercase-hex digest.
//
// Inputs are hardcoded so no stdin / fs is required. Each test case
// pairs an input string with its known SHA-256 digest; the program
// hashes each input and prints OK / FAIL.

const K: number[] = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
  0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
  0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
  0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
  0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
  0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

function rotr(x: number, n: number): number {
  return ((x >>> n) | (x << (32 - n))) >>> 0;
}

function shr(x: number, n: number): number {
  return (x >>> n) >>> 0;
}

function ch(x: number, y: number, z: number): number {
  return ((x & y) ^ (~x & z)) >>> 0;
}

function maj(x: number, y: number, z: number): number {
  return ((x & y) ^ (x & z) ^ (y & z)) >>> 0;
}

function bigSig0(x: number): number {
  return (rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22)) >>> 0;
}

function bigSig1(x: number): number {
  return (rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25)) >>> 0;
}

function smallSig0(x: number): number {
  return (rotr(x, 7) ^ rotr(x, 18) ^ shr(x, 3)) >>> 0;
}

function smallSig1(x: number): number {
  return (rotr(x, 17) ^ rotr(x, 19) ^ shr(x, 10)) >>> 0;
}

// Pre-process: convert input bytes (one per `number`, 0–255) to a
// padded message schedule, then run the compression function over
// each 512-bit block. Returns the eight 32-bit hash words.
function sha256Bytes(bytes: number[]): number[] {
  const len = bytes.length;
  const bitLen = len * 8;
  // Padding: append 0x80, then zeros, then 64-bit big-endian bit length.
  const padded: number[] = [];
  for (let i = 0; i < len; i++) {
    padded.push(bytes[i] & 0xff);
  }
  padded.push(0x80);
  while (padded.length % 64 !== 56) {
    padded.push(0);
  }
  // 64-bit big-endian length. JS numbers are f64 so > 2^32 bits is
  // representable up to 2^53 — we cover the high word as 0 since
  // tr's tests stay well under 2^32 bits.
  padded.push(0);
  padded.push(0);
  padded.push(0);
  padded.push(0);
  padded.push((bitLen >>> 24) & 0xff);
  padded.push((bitLen >>> 16) & 0xff);
  padded.push((bitLen >>> 8) & 0xff);
  padded.push(bitLen & 0xff);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;

  const w: number[] = [];
  for (let i = 0; i < 64; i++) w.push(0);

  const blocks = padded.length / 64;
  for (let b = 0; b < blocks; b++) {
    const base = b * 64;
    for (let t = 0; t < 16; t++) {
      const o = base + t * 4;
      w[t] = ((padded[o] << 24) | (padded[o + 1] << 16) | (padded[o + 2] << 8) | padded[o + 3]) >>> 0;
    }
    for (let t = 16; t < 64; t++) {
      w[t] = ((smallSig1(w[t - 2]) + w[t - 7] + smallSig0(w[t - 15]) + w[t - 16]) >>> 0);
    }
    let a = h0;
    let bb = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let hh = h7;
    for (let t = 0; t < 64; t++) {
      const t1 = (hh + bigSig1(e) + ch(e, f, g) + K[t] + w[t]) >>> 0;
      const t2 = (bigSig0(a) + maj(a, bb, c)) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = bb;
      bb = a;
      a = (t1 + t2) >>> 0;
    }
    h0 = (h0 + a) >>> 0;
    h1 = (h1 + bb) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
    h5 = (h5 + f) >>> 0;
    h6 = (h6 + g) >>> 0;
    h7 = (h7 + hh) >>> 0;
  }
  return [h0, h1, h2, h3, h4, h5, h6, h7];
}

function toHex8(n: number): string {
  const hex = "0123456789abcdef";
  let out = "";
  for (let i = 7; i >= 0; i--) {
    const nibble = (n >>> (i * 4)) & 0xf;
    out = out + hex[nibble];
  }
  return out;
}

function digestHex(words: number[]): string {
  let s = "";
  for (let i = 0; i < words.length; i++) {
    s = s + toHex8(words[i]);
  }
  return s;
}

function strToBytes(s: string): number[] {
  // ASCII-only conversion. tr's String stores bytes — charCodeAt
  // returns the byte value directly for codepoints < 128, which
  // covers every test input below.
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) {
    out.push(s.charCodeAt(i) & 0xff);
  }
  return out;
}

function sha256Of(s: string): string {
  return digestHex(sha256Bytes(strToBytes(s)));
}

// Known-answer tests — values from the NIST FIPS 180-4 examples and
// common references.
const empty = sha256Of("");
console.log(empty === "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" ? "OK empty" : "FAIL empty: " + empty);

const abc = sha256Of("abc");
console.log(abc === "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" ? "OK abc" : "FAIL abc: " + abc);

const longer = sha256Of("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
console.log(longer === "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1" ? "OK longer" : "FAIL longer: " + longer);
