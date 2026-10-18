// different: hand-written matcher for a.+c (s flag); no regex engine, no compiled automaton
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "pre a\nmiddle\nc post "

// hand-written matcher for /a.+c/s, not a regex engine
func find(s string) (int, int, bool) {
	// greedy `.+` backs off to the last `c` that leaves at least one byte after the `a`
	for a := 0; a < len(s); a++ {
		if s[a] != 'a' {
			continue
		}
		c := len(s)
		for c > a+2 {
			c--
			if s[c] == 'c' {
				return a, c + 1, true
			}
		}
	}
	return 0, 0, false
}

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var buf []byte
	for i := int64(0); i < n; i++ {
		buf = append(buf[:0], prefix...)
		buf = strconv.AppendInt(buf, i, 10)
		s := string(buf)
		if a, b, ok := find(s); ok {
			m0 := strings.Clone(s[a:b])
			total += int64(len(m0))
		}
	}
	fmt.Println(total)
}
