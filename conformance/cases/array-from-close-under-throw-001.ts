// Array.from closes the iterator when storing or mapping an element
// throws: the iterator's return() runs in full, and the original
// throw is what escapes (§7.4.11 IfAbruptCloseIterator).
let closed = 0
function note(msg: string): void {
  closed = closed + 1
  console.log('return ran', msg, closed)
}
function source(): any {
  const items: any = {}
  items[Symbol.iterator] = function () {
    return {
      return: function () {
        note('close')
        return {}
      },
      next: function () {
        return { done: false, value: 1 }
      },
    }
  }
  return items
}
const frozenCtor: any = function () {
  return Object.freeze([])
}
try {
  Array.from.call(frozenCtor, source())
} catch (e) {
  console.log('store threw', e instanceof TypeError)
}
try {
  Array.from(source(), function () {
    throw new RangeError('map')
  })
} catch (e) {
  console.log('map threw', e instanceof RangeError)
}
console.log(closed)
