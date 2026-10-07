// shift on receivers the inline head bump must still treat per spec:
// a non-extensible array, a hole punched after an earlier shift moved
// the head, and pushes that wrap a folded buffer
const ne: number[] = [1, 2, 3]
Object.preventExtensions(ne)
console.log(ne.shift(), ne.length)
try { ne.push(9) } catch (e) { console.log((e as Error).name, ne.join(",")) }
const late: number[] = [1, 2, 3, 4, 5]
late.shift()
delete late[1]
console.log(late.shift(), late.length, 0 in late, late[0], late.shift())
const fold: number[] = [1, 2, 3, 4]
fold.shift(); fold.shift()
for (let i = 0; i < 3; i = i + 1) fold.push(10 + i)
console.log(fold.join(","), fold.shift(), fold.length)
