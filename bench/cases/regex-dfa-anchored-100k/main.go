// different: hand-written matcher for ^\w+; no regex engine, no compiled automaton
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "hello world "

func isAlnum(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
}

// hand-written matcher for /^\w+/, not a regex engine
func find(s string) (int, int, bool) {
	end := 0
	for end < len(s) && (isAlnum(s[end]) || s[end] == '_') {
		end++
	}
	if end == 0 {
		return 0, 0, false
	}
	return 0, end, true
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
