// A `+` with a short literal on one side hands the literal to its
// kernel by value. The other operand keeps every rule of the general
// concat: null and undefined read as their names, the result takes
// the wider of the two encodings, and a longer or empty literal still
// goes through its cell.

function show(label: string, s: string): void {
  let codes = ''
  for (let i = 0; i < s.length; i = i + 1) {
    codes = codes + s.charCodeAt(i).toString(16) + ' '
  }
  console.log(label, s.length, JSON.stringify(s), codes)
}

function pick(i: number): string {
  return i % 2 === 0 ? 'ab' : 'cde'
}

function wide(i: number): string {
  return i % 2 === 0 ? '中' : '文字'
}

// every literal length that fits in eight bytes, on either side
const lits: string[] = []
lits.push(pick(0) + '1')
lits.push(pick(0) + '12')
lits.push(pick(0) + '123')
lits.push(pick(0) + '1234')
lits.push(pick(0) + '12345')
lits.push(pick(0) + '123456')
lits.push(pick(0) + '1234567')
lits.push(pick(0) + '12345678')
lits.push(pick(0) + '123456789')
lits.push('1' + pick(1))
lits.push('12' + pick(1))
lits.push('123' + pick(1))
lits.push('1234' + pick(1))
lits.push('12345' + pick(1))
lits.push('123456' + pick(1))
lits.push('1234567' + pick(1))
lits.push('12345678' + pick(1))
lits.push('123456789' + pick(1))
console.log(lits.join(' '))

// null and undefined operands read as their names
const nul = null as any as string
const und = undefined as any as string
const few: string[] = ['x', 'y']
show('null+lit', nul + ',')
show('lit+null', ',' + nul)
show('undef+lit', und + '!')
show('lit+undef', '!' + und)
show('hole+lit', few[7] + ';')
show('lit+hole', ';' + few[7])

// the middle of a chain is null
show('chain', pick(0) + ',' + nul + ';' + pick(1) + '\n')
show('chain-undef', pick(1) + '<' + und + '>' + few[9] + '.')

// a UTF-16 operand and a Latin-1 literal, both orders
show('wide+lit', wide(0) + ', ')
show('lit+wide', ', ' + wide(1))
show('wide+8', wide(1) + '12345678')
show('8+wide', '12345678' + wide(0))

// a UTF-16 literal and a Latin-1 operand, both orders
show('narrow+wide-lit', pick(0) + '中')
show('wide-lit+narrow', '文字' + pick(1))
show('narrow+4', pick(1) + '中文测试')
show('5-units', pick(1) + '中文测试字')
show('wide+wide-lit', wide(1) + '测')
show('astral', pick(0) + '😀')
show('astral-pair', '😀😀' + wide(0))

// a non-ASCII Latin-1 literal stays Latin-1 and is one unit a byte
show('latin1-lit', pick(0) + 'é')
show('lit-latin1', 'ÿ¡' + pick(1))
show('latin1+wide', wide(0) + 'é')
console.log((pick(0) + 'é').toUpperCase(), ('ñ' + pick(1)).indexOf('c'))

// an empty literal adds nothing
show('empty-right', pick(1) + '')
show('empty-left', '' + pick(1))
show('empty-null', nul + '')
show('empty-wide', '' + wide(1))

// a number piece beside a literal piece
for (let i = 9; i < 12; i = i + 1) {
  show('num', i + ':' + pick(i) + '=' + i * 1.5 + ';')
}

// the literal operand is the same cell on both sides
const dash = '-'
show('lit+lit-cell', dash + '-' + dash)

// a view on the left or the right
const parts = 'alpha,beta,,中文,é'.split(',')
for (let i = 0; i < parts.length; i = i + 1) {
  show('view', parts[i] + '|')
  show('weiv', '|' + parts[i])
  show('v-wide', parts[i] + '终')
  show('wide-v', '始' + parts[i])
}
