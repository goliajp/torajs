class Tok { constructor(public k: number, public s: string) {} }
const n = parseInt(process.argv[2] ?? '40', 10)
const toks: Tok[] = []
const nums: number[] = []
const strs: string[] = []
const fl: number[] = []
for (let i = 0; i < n; i = i + 1) {
  toks.push(new Tok(i, 'w' + i))
  nums.push(i * 3)
  strs.push('s' + (i % 7))
  fl.push(i / 4)
  if (i % 9 === 4) nums.shift()
}
let acc = 0
for (let i = 0; i < toks.length; i = i + 1) acc = acc + toks[i].k + toks[i].s.length
console.log(toks.length, nums.length, strs.length, acc, nums[0], nums[nums.length - 1], strs.join('').length, fl[fl.length - 1])
const frozen: number[] = [1, 2, 3]
Object.freeze(frozen)
try { frozen.push(4) } catch (e) { console.log('frozen', (e as Error).name, frozen.length) }
const sealed: number[] = [1, 2]
Object.seal(sealed)
try { sealed.push(4) } catch (e) { console.log('sealed', (e as Error).name, sealed.length) }
const ne = [5]
Object.preventExtensions(ne)
try { ne.push(6) } catch (e) { console.log('nonext', (e as Error).name, ne.length) }
const ro = [1]
Object.defineProperty(ro, 'length', { writable: false })
try { ro.push(2) } catch (e) { console.log('ro', (e as Error).name, ro.length) }
