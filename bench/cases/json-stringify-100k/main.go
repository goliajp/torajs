// different: the serializer is fixed to the struct's keys and value kinds; JSON.stringify discovers both at run time
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Record struct {
	id     int64
	name   string
	score  int64
	active bool
}

func makeRecord(i int64) *Record {
	return &Record{id: i, name: "row", score: i * 7, active: (i & 1) == 0}
}

// QuoteJSONString: escape '"', '\\' and control chars
func quote(out *strings.Builder, s string) {
	out.WriteByte('"')
	for _, c := range s {
		switch {
		case c == '"':
			out.WriteString("\\\"")
		case c == '\\':
			out.WriteString("\\\\")
		case c == '\n':
			out.WriteString("\\n")
		case c == '\r':
			out.WriteString("\\r")
		case c == '\t':
			out.WriteString("\\t")
		case c == '\b':
			out.WriteString("\\b")
		case c == '\f':
			out.WriteString("\\f")
		case c < 0x20:
			fmt.Fprintf(out, "\\u%04x", c)
		default:
			out.WriteRune(c)
		}
	}
	out.WriteByte('"')
}

func writeInt(out *strings.Builder, v int64) {
	var buf [20]byte
	out.Write(strconv.AppendInt(buf[:0], v, 10))
}

func stringify(r *Record) string {
	var out strings.Builder
	out.WriteByte('{')
	quote(&out, "id")
	out.WriteByte(':')
	writeInt(&out, r.id)
	out.WriteByte(',')
	quote(&out, "name")
	out.WriteByte(':')
	quote(&out, r.name)
	out.WriteByte(',')
	quote(&out, "score")
	out.WriteByte(':')
	writeInt(&out, r.score)
	out.WriteByte(',')
	quote(&out, "active")
	out.WriteByte(':')
	if r.active {
		out.WriteString("true")
	} else {
		out.WriteString("false")
	}
	out.WriteByte('}')
	return out.String()
}

// a package-level sink keeps the record on the heap, as black_box keeps it in main.rs
var sinkRecord *Record

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for i := int64(0); i < n; i++ {
		sinkRecord = makeRecord(i)
		s := stringify(sinkRecord)
		total = total + int64(len(s))
	}
	fmt.Println(total)
}
