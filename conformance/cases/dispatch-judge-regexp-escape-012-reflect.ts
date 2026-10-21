// a RegExp handed to Reflect / Object statics
console.log(Reflect.get(/abc/, 'source'))
console.log(Object.keys(Object.assign({}, /x/)).length)
console.log(Object.prototype.toString.call(/y/))
const frozen = Object.freeze(/f/)
console.log(Object.isFrozen(frozen), frozen.test('f'))
