// A loop accumulator that starts as '' and appends only wide text is
// still empty when the loop runs zero times; joining it onto narrow
// text must leave that text narrow.
const z = Number(process.argv.length > 5 ? 1 : 0);

function w(n: number, t: string): string {
  let a = '';
  for (let i = 0; i < n; i++) a += '中';
  let o = t;
  o += a;
  return o;
}

console.log('<b>'.includes(w(0, 'b')), w(0, 'b') === 'b', w(2, 'b'));
const r = w(z, 'b');
console.log('<b>'.includes(r), r === 'b', r.length, '<b>'.indexOf(r), w(z + 2, 'b'));
const u = w(z, 'b' + z);
console.log('<b0>'.includes(u), u === 'b0', '<b0>'.indexOf(u), u.length);

// zero trips, the accumulator only read
function readOnly(n: number): void {
  let a = '';
  for (let i = 0; i < n; i++) a += '中';
  console.log(a === '', a.length, 'x'.indexOf(a), 'x'.includes(a), 'x'.startsWith(a), 'x'.endsWith(a));
}
readOnly(z);
readOnly(z + 1);
let top = '';
for (let i = 0; i < z; i++) top += '中';
console.log(top === '', top.length, 'x'.indexOf(top), '' === top);

// mixed widths
function mixed(n: number): void {
  let a = '';
  for (let i = 0; i < n; i++) a += i % 2 ? 'é' : '中';
  console.log(a.length, a.charCodeAt(n - 1), a.charCodeAt(0), a.endsWith('中é'));
}
mixed(40);
mixed(z + 40);
let m = '';
for (let i = 0; i < 40; i++) m += i % 2 ? 'é' : '中';
console.log(m.length, m.charCodeAt(39));

// an outer loop that starts the accumulator over each time
for (let j = 0; j < 3; j++) {
  let a = '';
  for (let i = 0; i < 50; i++) a += 'ab';
  console.log(a.length);
}
function outer(k: number): void {
  for (let j = 0; j < k; j++) {
    let a = '';
    for (let i = 0; i < 50 + j; i++) a += j % 2 ? '中' : 'ab';
    console.log(a.length, a.charCodeAt(a.length - 1));
  }
}
outer(z + 3);
