"""Pull the Digital Link vectors out of gs1-syntax-engine's dl.c.

Three blocks matter, and they pin different things:

  test_URIescape(in, path, query)     — how a value is escaped, and the one
                                        place the two components disagree
  test_URIunescape(in, path, query)   — the reverse, including the truncated
                                        escapes that stay literal
  test_dl_gs1_parse(uri, elements)    — a whole URI to an element string

They are written as C macros with string literals, so the literals have to be
decoded the same way `extract-vectors.py` decodes the linter ones.

Usage:  python3 extract-dl-vectors.py /path/to/dl.c out.txt
"""

import os
import re
import sys

sys.dont_write_bytecode = True  # this directory is ours, but keep it clean
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import provenance  # noqa: E402

SRC = sys.argv[1]
OUT = sys.argv[2]

ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0', '\\': '\\', '"': '"',
           "'": "'", 'a': '\a', 'b': '\b', 'f': '\f', 'v': '\v'}


def read_literal(text, i):
    """Decode a C string literal whose opening quote is just before `i`.

    Adjacent literals concatenate, as C says they do: "A" "\\x01" "B" is one
    string, and dl.c relies on that to write a control character in the middle
    of a word without ending the escape early.
    """
    out = []
    while True:
        while i < len(text):
            c = text[i]
            if c == '"':
                i += 1
                break
            if c == '\\':
                nxt = text[i + 1]
                if nxt == 'x':
                    j = i + 2
                    while j < len(text) and text[j] in '0123456789abcdefABCDEF':
                        j += 1
                    out.append(chr(int(text[i + 2:j], 16)))
                    i = j
                    continue
                if nxt in ESCAPES:
                    out.append(ESCAPES[nxt])
                    i += 2
                    continue
                raise SystemExit(f"unknown escape \\{nxt}")
            out.append(c)
            i += 1
        # Another literal right after this one? Then it is the same string.
        j = i
        while j < len(text) and text[j] in ' \t\n':
            j += 1
        if j < len(text) and text[j] == '"':
            i = j + 1
            continue
        return ''.join(out), i


def calls(text, name, arity):
    """Every `name(a, b, ...)` call with `arity` string-literal arguments."""
    pattern = re.compile(re.escape(name) + r'\(\s*"')
    for match in pattern.finditer(text):
        i = match.end()
        args = []
        try:
            for n in range(arity):
                value, i = read_literal(text, i)
                args.append(value)
                if n + 1 < arity:
                    # Skip ", and the opening quote of the next argument.
                    while text[i] in ' \t\n,':
                        i += 1
                    if text[i] != '"':
                        raise ValueError
                    i += 1
        except (IndexError, ValueError):
            continue
        yield args


def escaped(value):
    """Write a value so a tab-separated file can carry it."""
    return (value.replace('\\', '\\\\')
                 .replace('\t', '\\t')
                 .replace('\n', '\\n'))


def main():
    with open(SRC, encoding='utf-8') as handle:
        text = handle.read()
    # Drop the macro definitions themselves; only the call sites are vectors.
    text = re.sub(r'#define\s+test_URI(un)?escape.*?\n\n', '', text,
                  flags=re.S)

    rows = []
    for kind, arity in (('escape', 3), ('unescape', 3)):
        for args in calls(text, f'test_URI{kind}', arity):
            rows.append((kind, *args))

    with open(OUT, 'w', encoding='utf-8') as handle:
        handle.write(
            "# GS1 Digital Link vectors, extracted from gs1-syntax-engine's"
            " dl.c\n"
            "# by crates/gs1-syntax-gen/extract-dl-vectors.py. Do not edit.\n"
            f"# Derived from {provenance.stamp(SRC, [SRC])}\n"
            "#\n"
            "# kind\tinput\tas a path\tas a query\n")
        for row in rows:
            handle.write('\t'.join(escaped(c) for c in row) + '\n')
    print(f'{len(rows)} vectors -> {OUT}')


if __name__ == '__main__':
    main()
