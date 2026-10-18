// different: hand-coded forward scan for a(b+)c stands in for a regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

// hand-coded matcher for /a(b+)c/g: b+ is greedy and backtracking can only
// hand a 'b' to c, which never matches, so one forward scan is exact
func replaceABC(s string, with string) string {
	var out strings.Builder
	i := 0
	last := 0
	for i < len(s) {
		if s[i] == 'a' {
			j := i + 1
			for j < len(s) && s[j] == 'b' {
				j++
			}
			if j > i+1 && j < len(s) && s[j] == 'c' {
				out.WriteString(s[last:i])
				out.WriteString(with)
				i = j + 1
				last = i
				continue
			}
		}
		i++
	}
	out.WriteString(s[last:])
	return out.String()
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	words := strings.Split(os.Args[2], ",")
	subjects := make([]string, 0, 16)
	for k := 0; k < 16; k++ {
		row := make([]string, 0, len(words))
		for w := 0; w < len(words); w++ {
			row = append(row, words[(w+k)%len(words)])
		}
		subjects = append(subjects, strings.Join(row, " "))
	}
	var total int64 = 0
	last := ""
	for i := int64(0); i < n; i++ {
		r := replaceABC(subjects[i&15], "XY")
		total = total + int64(len(r)) + int64(r[i%int64(len(r))])
		last = r
	}
	fmt.Println(last)
	fmt.Println(total)
}
