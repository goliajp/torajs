package main

import (
	"fmt"
	"os"
	"strconv"
)

func next(s int64) int64 {
	t := int64(int32(s*48271)) & 0x7fffffff
	if t == 0 {
		return 1
	}
	return t
}

var seed int64 = 1

func gen(depth int64) string {
	seed = next(seed)
	r := seed % 6
	if depth == 0 || r == 0 {
		seed = next(seed)
		if seed&1 == 0 {
			return strconv.FormatInt(seed%1000, 10)
		}
		return string(rune(97 + ((seed >> 1) & 7)))
	}
	if r == 1 {
		return "-" + gen(depth-1)
	}
	if r == 2 {
		return "(" + gen(depth-1) + ")"
	}
	left := gen(depth - 1)
	seed = next(seed)
	op := seed % 3
	right := gen(depth - 1)
	if op == 0 {
		return left + " + " + right
	}
	if op == 1 {
		return left + " - " + right
	}
	return left + " * " + right
}

const (
	NUM    = 0
	IDENT  = 1
	PLUS   = 2
	MINUS  = 3
	STAR   = 4
	LPAREN = 5
	RPAREN = 6
	END    = 7
	NEG    = 8
)

type Token struct {
	kind  int32
	value int32
}

func tokenize(src string) []*Token {
	out := []*Token{}
	i := 0
	for i < len(src) {
		c := src[i]
		if c == 32 {
			i++
		} else if c >= 48 && c <= 57 {
			var v int32
			for i < len(src) {
				d := src[i]
				if d < 48 || d > 57 {
					break
				}
				v = v*10 + int32(d-48)
				i++
			}
			out = append(out, &Token{NUM, v})
		} else if c >= 97 && c <= 122 {
			out = append(out, &Token{IDENT, int32(c - 97)})
			i++
		} else {
			var kind int32 = RPAREN
			if c == 43 {
				kind = PLUS
			} else if c == 45 {
				kind = MINUS
			} else if c == 42 {
				kind = STAR
			} else if c == 40 {
				kind = LPAREN
			}
			out = append(out, &Token{kind, 0})
			i++
		}
	}
	out = append(out, &Token{END, 0})
	return out
}

type Expr struct {
	kind  int32
	value int32
	left  *Expr
	right *Expr
}

var nodes int64

type Parser struct {
	toks []*Token
	pos  int
}

func (p *Parser) expr() *Expr {
	left := p.term()
	for p.toks[p.pos].kind == PLUS || p.toks[p.pos].kind == MINUS {
		kind := p.toks[p.pos].kind
		p.pos++
		right := p.term()
		nodes++
		left = &Expr{kind, 0, left, right}
	}
	return left
}

func (p *Parser) term() *Expr {
	left := p.factor()
	for p.toks[p.pos].kind == STAR {
		p.pos++
		right := p.factor()
		nodes++
		left = &Expr{STAR, 0, left, right}
	}
	return left
}

func (p *Parser) factor() *Expr {
	t := p.toks[p.pos]
	p.pos++
	nodes++
	if t.kind == NUM {
		return &Expr{NUM, t.value, nil, nil}
	}
	if t.kind == IDENT {
		return &Expr{IDENT, t.value, nil, nil}
	}
	if t.kind == MINUS {
		return &Expr{NEG, 0, p.factor(), nil}
	}
	inner := p.expr()
	p.pos++
	nodes--
	return inner
}

func evaluate(e *Expr, env []int32) int32 {
	if e.kind == NUM {
		return e.value
	}
	if e.kind == IDENT {
		return env[e.value]
	}
	if e.kind == NEG {
		return 0 - evaluate(e.left, env)
	}
	a := evaluate(e.left, env)
	b := evaluate(e.right, env)
	if e.kind == PLUS {
		return a + b
	}
	if e.kind == MINUS {
		return a - b
	}
	return a * b
}

func main() {
	lineCount, _ := strconv.ParseInt(os.Args[1], 10, 64)
	depth, _ := strconv.ParseInt(os.Args[2], 10, 64)
	rounds, _ := strconv.ParseInt(os.Args[3], 10, 64)
	seed, _ = strconv.ParseInt(os.Args[4], 10, 64)

	lines := []string{}
	for i := int64(0); i < lineCount; i++ {
		lines = append(lines, gen(depth))
	}

	env := []int32{3, 5, 7, 11, 13, 17, 19, 23}
	var total int32
	var tokens int64
	for r := int64(0); r < rounds; r++ {
		for i := 0; i < len(lines); i++ {
			toks := tokenize(lines[i])
			tokens += int64(len(toks))
			p := &Parser{toks: toks}
			total += evaluate(p.expr(), env)
		}
		env[r&7] += total
	}
	fmt.Println(total, tokens, nodes)
}
