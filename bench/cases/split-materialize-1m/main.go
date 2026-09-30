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

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	fields, _ := strconv.ParseInt(os.Args[2], 10, 64)
	seed, _ := strconv.ParseInt(os.Args[3], 10, 64)

	lines := []string{}
	for i := 0; i < 1024; i++ {
		var line strings.Builder
		for f := int64(0); f < fields; f++ {
			seed = next(seed)
			if f > 0 {
				line.WriteByte(',')
			}
			line.WriteString(strconv.FormatInt(seed%100000, 10))
		}
		lines = append(lines, line.String())
	}

	var total int64
	for i := int64(0); i < n; i++ {
		parts := strings.Split(lines[i&1023], ",")
		var rebuilt strings.Builder
		for k := 0; k < len(parts); k++ {
			p := parts[k]
			total += int64(p[len(p)-1])
			rebuilt.WriteString(p)
			rebuilt.WriteByte(';')
		}
		total += int64(len(rebuilt.String()))
	}
	fmt.Println(total)
}
