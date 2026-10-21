"""What the extraction scripts stamp on the files they derive.

The pinned engine commit lives in ../gs1-syntax/data/UPSTREAM; the copyright
years are read from the sources actually being extracted, so a header can
only say what those files say. The scripts refuse to run on a checkout that
is not the pinned commit, because a header naming one commit above data from
another is worse than no header.
"""

import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
UPSTREAM = os.path.join(HERE, '..', 'gs1-syntax', 'data', 'UPSTREAM')
YEARS = re.compile(r'Copyright \(c\) (\d{4})(?:-(\d{4}))? GS1 AISBL')


def pinned():
    """The full SHA `engine.commit` in data/UPSTREAM names."""
    for line in open(UPSTREAM, encoding='utf-8'):
        key, _, value = line.partition('=')
        if key.strip() == 'engine.commit':
            return value.strip()
    raise SystemExit(f'{UPSTREAM}: no engine.commit line')


def checked_out(start):
    """The commit of the git checkout holding `start`, or None for a tarball."""
    d = os.path.abspath(start)
    while not os.path.exists(os.path.join(d, '.git')):
        parent = os.path.dirname(d)
        if parent == d:
            return None
        d = parent
    git = os.path.join(d, '.git')
    head = open(os.path.join(git, 'HEAD'), encoding='utf-8').read().strip()
    if not head.startswith('ref: '):
        return head
    ref = head[5:]
    try:
        return open(os.path.join(git, ref), encoding='utf-8').read().strip()
    except OSError:
        for line in open(os.path.join(git, 'packed-refs'), encoding='utf-8'):
            if line.strip().endswith(' ' + ref):
                return line.split()[0]
    return None


def stamp(start, sources):
    """The one-line provenance for files derived from `sources`.

    `start` is any path inside the engine checkout; `sources` are the files
    whose copyright headers set the years (earliest start to latest end).
    """
    sha = pinned()
    got = checked_out(start)
    if got is not None and got != sha:
        raise SystemExit(f'engine checkout is at {got}, UPSTREAM pins {sha}')
    years = []
    for path in sources:
        m = YEARS.search(open(path, encoding='utf-8').read(2000))
        if not m:
            raise SystemExit(f'{path}: no GS1 copyright line in its header')
        years += [int(y) for y in m.groups() if y]
    return (f'gs1-syntax-engine {sha[:7]}, Copyright (c) {min(years)}-{max(years)} '
            'GS1 AISBL, licensed under the Apache License 2.0; see NOTICE.')
