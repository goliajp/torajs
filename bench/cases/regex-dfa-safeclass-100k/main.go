// different: hand-written byte-class scan for [A-Za-z0-9]+; no regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "  Hello42 world  "

func isAlnum(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
}

// hand-written matcher for /[A-Za-z0-9]+/, not a regex engine
func find(s string) (int, int, bool) {
	a := 0
	for a < len(s) && !isAlnum(s[a]) {
		a++
	}
	if a == len(s) {
		return 0, 0, false
	}
	end := a + 1
	for end < len(s) && isAlnum(s[end]) {
		end++
	}
	return a, end, true
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
