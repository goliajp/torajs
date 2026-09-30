package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

func next(s int64) int64 {
	t := (s * 48271) % 2147483647
	if t == 0 {
		return 1
	}
	return t
}

func isLower(c byte) bool {
	return c >= 'a' && c <= 'z'
}

func isDigit(c byte) bool {
	return c >= '0' && c <= '9'
}

// ([a-z]+)=(\d+): every start inside one run of letters reaches the same end,
// so a run that fails is skipped whole
func find(s string, from int) (index, keyEnd, end int, ok bool) {
	p := from
	for p < len(s) {
		if !isLower(s[p]) {
			p++
			continue
		}
		q := p + 1
		for q < len(s) && isLower(s[q]) {
			q++
		}
		if q+1 < len(s) && s[q] == '=' && isDigit(s[q+1]) {
			r := q + 2
			for r < len(s) && isDigit(s[r]) {
				r++
			}
			return p, q, r, true
		}
		p = q
	}
	return 0, 0, 0, false
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	fields, _ := strconv.ParseInt(os.Args[2], 10, 64)
	seed, _ := strconv.ParseInt(os.Args[3], 10, 64)

	lines := []string{}
	for i := 0; i < 1024; i++ {
		var line strings.Builder
		for f := int64(0); f < fields; f++ {
			seed = next(seed)
			shape := seed % 8
			seed = next(seed)
			klen := 1 + seed%8
			var key strings.Builder
			for k := int64(0); k < klen; k++ {
				seed = next(seed)
				key.WriteByte(byte('a' + seed%26))
			}
			seed = next(seed)
			vlen := 1 + seed%6
			var value strings.Builder
			for k := int64(0); k < vlen; k++ {
				seed = next(seed)
				value.WriteByte(byte('0' + seed%10))
			}
			if shape != 1 {
				line.WriteString(key.String())
			}
			if shape != 0 {
				line.WriteByte('=')
			}
			line.WriteString(value.String())
			line.WriteByte(';')
		}
		lines = append(lines, line.String())
	}

	var total int64
	for i := int64(0); i < n; i++ {
		line := lines[i&1023]
		lastIndex := 0
		for {
			index, keyEnd, end, ok := find(line, lastIndex)
			if !ok {
				break
			}
			key := strings.Clone(line[index:keyEnd])
			value := strings.Clone(line[keyEnd+1 : end])
			v, _ := strconv.ParseInt(value, 10, 64)
			total += int64(index) + int64(len(key)) + v
			lastIndex = end
		}
	}
	fmt.Println(total)
}
