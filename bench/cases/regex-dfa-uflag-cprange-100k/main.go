// different: hand-written byte-class scan for [A-Za-z]+ (u flag); no regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "  Hello42 word  "

func isAlpha(c byte) bool {
	return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
}

// hand-written matcher for /[A-Za-z]+/u, not a regex engine; an ascii-only class never
// matches a utf-8 lead or continuation byte, so stepping bytes gives the u-flag matches
func find(s string) (int, int, bool) {
	a := 0
	for a < len(s) && !isAlpha(s[a]) {
		a++
	}
	if a == len(s) {
		return 0, 0, false
	}
	end := a + 1
	for end < len(s) && isAlpha(s[end]) {
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
