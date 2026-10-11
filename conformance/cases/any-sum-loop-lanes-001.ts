// An `any` accumulator a loop adds `any` elements into: int32 and
// double encodings alternating, the int32 range crossed and come
// back to, -0 and NaN kept, a string or undefined addend partway,
// the loop left early by break and return, an addend whose valueOf
// throws, a total the loop also hands to a call, a nested loop and a
// total entered as an unknown value.

const f64 = new Float64Array(1)
const u32 = new Uint32Array(f64.buffer)
u32[1] = 0xfff80001
const oddNaN: any = f64[0]

function show(label: string, v: any) {
  console.log(label, String(v), typeof v, Object.is(v, -0) ? '-0' : '', v !== v ? 'nan' : '')
}

// int32 and double alternating, the int32 range crossed
{
  const xs: any[] = [1, 0.5, -1, 2147483647, 1, -1, -2147483648, -1, 1, 0.25]
  let total = 0
  for (let i = 0; i < 40; i = i + 1) total = total + xs[i % xs.length]
  show('alt', total)
}

// every element an int32: the sum leaves and re-enters int32 range
{
  const xs: any[] = [2147483647, 1, -1, -1]
  let total = 0
  for (let i = 0; i < 400; i = i + 1) total = total + xs[i % 4]
  show('int', total)
}

// -0 stays -0 while every addend is -0; one +0 ends it
{
  const zs: any[] = [-0, -0, -0]
  let acc: any = -0
  for (let i = 0; i < 9; i = i + 1) acc = acc + zs[i % 3]
  show('negzero', acc)
  acc = acc + 0
  show('negzero+0', acc)
  let fromZero = 0
  for (let i = 0; i < 3; i = i + 1) fromZero = fromZero + zs[i]
  show('zero+negzero', fromZero)
}

// NaN addends, one of them a NaN whose payload would collide
{
  const ns: any[] = [1, NaN, 2, oddNaN]
  let total = 0
  for (let i = 0; i < 8; i = i + 1) total = total + ns[i % 4]
  show('nan', total)
  const back = new Float64Array(1)
  back[0] = total
  const words = new Uint32Array(back.buffer)
  console.log('nan words', (words[1] & 0x7ff00000) === 0x7ff00000)
}

// a string partway turns the rest into concatenation
{
  const ss: any[] = [1, 2.5, 'x', 3, 4.5]
  let total = 0
  for (let i = 0; i < 10; i = i + 1) total = total + ss[i % 5]
  show('string', total)
}

// undefined and null partway
{
  const us: any[] = [1, 2, undefined, 3]
  let total = 0
  for (let i = 0; i < 8; i = i + 1) total = total + us[i % 4]
  show('undefined', total)
  const nulls: any[] = [1, null, 2.5]
  let t2 = 0
  for (let i = 0; i < 6; i = i + 1) t2 = t2 + nulls[i % 3]
  show('null', t2)
}

// booleans and objects with valueOf
{
  const bs: any[] = [true, 1.5, false, { valueOf: () => 40 }, [5]]
  let total = 0
  for (let i = 0; i < 10; i = i + 1) total = total + bs[i % 5]
  show('coerce', total)
}

// break and return leave the loop with the sum so far
{
  const xs: any[] = [0.5, 1, 1.5, 2]
  let total = 0
  for (let i = 0; i < 100; i = i + 1) {
    total = total + xs[i % 4]
    if (total > 20) break
  }
  show('break', total)
  function upTo(limit: number): any {
    let t = 0
    for (let i = 0; i < 100; i = i + 1) {
      t = t + xs[i % 4]
      if (t > limit) return t
    }
    return t
  }
  show('return', upTo(7))
  show('return-end', upTo(1e9))
}

// an addend whose valueOf throws: the sum so far is observable
{
  const bad: any = {
    valueOf: () => {
      throw new RangeError('no')
    },
  }
  const xs: any[] = [1, 2.5, bad]
  let total = 0
  try {
    for (let i = 0; i < 10; i = i + 1) total = total + xs[i % 3]
  } catch (e) {
    console.log('threw', e instanceof RangeError, String(total))
  }
  let inner = 0
  for (let i = 0; i < 6; i = i + 1) {
    try {
      inner = inner + xs[i % 3]
    } catch (e) {
      inner = inner + 100
    }
  }
  show('caught-inside', inner)
}

// bigint addend: TypeError out of the kernel
{
  const xs: any[] = [1, 10n]
  let total = 0.5
  try {
    for (let i = 0; i < 4; i = i + 1) total = total + xs[i % 2]
  } catch (e) {
    console.log('bigint', e instanceof TypeError, String(total))
  }
}

// the total handed to a call inside the loop
{
  const xs: any[] = [1, 0.5]
  let total = 0
  let seen = ''
  for (let i = 0; i < 6; i = i + 1) {
    total = total + xs[i % 2]
    seen = seen + String(total) + ','
  }
  console.log('escape', seen)
}

// nested loops, the inner total folded into the outer one
{
  const xs: any[] = [0.5, 1, 1.5]
  let outer = 0
  for (let i = 0; i < 4; i = i + 1) {
    let inner = 0
    for (let j = 0; j < 6; j = j + 1) inner = inner + xs[(i + j) % 3]
    outer = outer + inner
  }
  show('nested', outer)
}

// a total that enters as an unknown value: a number, then a string
{
  function seed(k: number): any {
    return k > 0 ? 0.25 : 'seed'
  }
  const xs: any[] = [1, 2]
  for (const k of [1, 0]) {
    let total = seed(k)
    for (let i = 0; i < 4; i = i + 1) total = total + xs[i % 2]
    show('seed' + k, total)
  }
}

// an int32 total that stays int32: the result prints as an integer
{
  const xs: any[] = [3, -1]
  let total = 0
  for (let i = 0; i < 1000; i = i + 1) total = total + xs[i % 2]
  show('stays-int', total)
  console.log(Number.isInteger(total), (total | 0) === total)
}
