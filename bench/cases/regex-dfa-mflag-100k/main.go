// different: hand-written matcher for ^line (m flag); no regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "pre\nline-text\nend "

// hand-written matcher for /^line/m, not a regex engine
func lineStart(s string, a int) bool {
	// js line terminators: \n, \r, U+2028, U+2029
	return a == 0 ||
		s[a-1] == '\n' ||
		s[a-1] == '\r' ||
		(a >= 3 && s[a-3] == 0xE2 && s[a-2] == 0x80 && (s[a-1] == 0xA8 || s[a-1] == 0xA9))
}

func find(s string) (int, int, bool) {
	const pat = "line"
	if len(s) < len(pat) {
		return 0, 0, false
	}
	for a := 0; a <= len(s)-len(pat); a++ {
		if lineStart(s, a) && s[a:a+len(pat)] == pat {
			return a, a + len(pat), true
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
