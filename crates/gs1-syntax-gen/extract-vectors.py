"""Pull the UNIT_TEST vectors out of gs1-syntax-engine's linter sources.

Each lint_*.c ends in a block of UNIT_TEST_PASS(fn, "value") and
UNIT_TEST_FAIL(fn, "value", ERR, "highlight") calls. We want the linter name,
the verdict and the value, with the C string literal decoded.
"""

import glob
import os
import re
import sys

sys.dont_write_bytecode = True  # this directory is ours, but keep it clean
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import provenance  # noqa: E402

SRC = sys.argv[1]
OUT = sys.argv[2]

CALL = re.compile(r'UNIT_TEST_(PASS|FAIL)\(gs1_lint_([a-z0-9_]+),\s*"')

ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0', '\\': '\\', '"': '"',
           "'": "'", 'a': '\a', 'b': '\b', 'f': '\f', 'v': '\v'}


def read_literal(text, i):
    """Decode a C string literal whose opening quote is just before `i`."""
    out = []
    while i < len(text):
        c = text[i]
        if c == '"':
            return ''.join(out), i + 1
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
            raise ValueError(f'unknown escape \\{nxt}')
        out.append(c)
        i += 1
    raise ValueError('unterminated literal')


# The engine spells the hook two ways: GS1_LINTER_CUSTOM_GCP_LOOKUP_H names the
# header to include, GS1_LINTER_CUSTOM_GCP_LOOKUP the lookup macro itself, and
# lint_gcppos2.c guards its vectors with only the latter. One is a prefix of
# the other, so this matches both.
GCP_HOOK = '#ifdef GS1_LINTER_CUSTOM_GCP_LOOKUP'

rows = []
for path in sorted(glob.glob(os.path.join(SRC, 'lint_*.c'))):
    body = open(path, encoding='utf-8').read()
    # Vectors behind the GCP-lookup hook only hold when a caller has compiled
    # in a company-prefix lookup service. They contradict the ones in front of
    # it — the same value appears as both PASS and FAIL — so a build without
    # that service, which is ours, must not read them.
    guard = body.rfind(GCP_HOOK)
    if guard != -1:
        body = body[:guard]
    # The vectors live in the #ifdef UNIT_TESTS block at the end.
    for m in CALL.finditer(body):
        verdict, name = m.group(1).lower(), m.group(2)
        try:
            value, i = read_literal(body, m.end())
            # C concatenates adjacent string literals, and GS1 splits its long
            # vectors across lines that way. Keep reading while the next
            # non-space character opens another literal.
            while True:
                j = i
                while j < len(body) and body[j] in ' \t\r\n':
                    j += 1
                if j >= len(body) or body[j] != '"':
                    break
                more, i = read_literal(body, j + 1)
                value += more
        except ValueError as e:
            print(f'{name}: {e}', file=sys.stderr)
            continue
        rows.append((name, verdict, value))

# One vector per line: name, verdict, then the value with the few characters
# that would break the format written as \xNN.
def quote(s):
    out = []
    for ch in s:
        if ch == '\\':
            out.append('\\\\')
        elif ch == '\n' or ch == '\t' or ch == '\r' or ord(ch) < 0x20 or ord(ch) == 0x7f:
            out.append('\\x%02x' % ord(ch))
        else:
            out.append(ch)
    return ''.join(out)


sources = sorted(glob.glob(os.path.join(SRC, 'lint_*.c')))
with open(OUT, 'w', encoding='utf-8') as f:
    f.write("# GS1 linter vectors, extracted from gs1-syntax-engine's lint_*.c\n"
            '# by crates/gs1-syntax-gen/extract-vectors.py. Do not edit.\n'
            f'# Derived from {provenance.stamp(SRC, sources)}\n'
            '#\n'
            '# linter\tverdict\tvalue\n')
    for name, verdict, value in rows:
        f.write(f'{name}\t{verdict}\t{quote(value)}\n')

names = sorted({r[0] for r in rows})
print(f'{len(rows)} vectors, {len(names)} linters')
print(' '.join(names))
