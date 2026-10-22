"""Vendor the newest GS1 Barcode Syntax Dictionary from upstream's main branch.

    python3 crates/gs1-syntax-gen/update-dictionary.py [--body FILE]

Asks GitHub for the last commit on gs1/gs1-syntax-dictionary's main that
touched gs1-syntax-dictionary.txt and compares that file's SHA-256 with the
one recorded in ../gs1-syntax/data/UPSTREAM. When they match there is nothing
to do. When they differ it replaces the vendored file byte for byte, rewrites
the three `dictionary.*` lines of UPSTREAM, and moves the commit, date and
copyright lines both NOTICE files carry for the dictionary. It does not
regenerate ai.rs; run `cargo run -p gs1-syntax-gen && cargo fmt --all` after.

`dictionary.release` is the release name when the file carries one in its
`# Release:` line (or the commit is the tagged release itself), and otherwise
names the commit and how far past the last release it is. The prose in NOTICE
that says what the snapshot contains is left for a person to rewrite — the
script lists it in the summary instead of guessing.

`--body FILE` writes a Markdown summary for a pull request. Under GitHub
Actions the result also goes to $GITHUB_OUTPUT (`changed`, `commit`, `short`,
`date`, `release`, `previous`). GITHUB_TOKEN or GH_TOKEN, when set, is sent
with the API requests to lift the anonymous rate limit.
"""

import argparse
import hashlib
import json
import os
import re
import sys
import urllib.request

REPO = 'gs1/gs1-syntax-dictionary'
FILE = 'gs1-syntax-dictionary.txt'
BRANCH = 'main'

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, '..', 'gs1-syntax', 'data')
UPSTREAM = os.path.join(DATA, 'UPSTREAM')
VENDORED = os.path.join(DATA, FILE)
NOTICES = [os.path.join(HERE, '..', '..', 'NOTICE'),
           os.path.join(HERE, '..', 'gs1-syntax', 'NOTICE')]
TAG = re.compile(r'^\d{4}-\d{2}-\d{2}$')


def fetch(url):
    headers = {'User-Agent': 'lib-gs1-syntax/update-dictionary'}
    token = os.environ.get('GITHUB_TOKEN') or os.environ.get('GH_TOKEN')
    if token and url.startswith('https://api.github.com/'):
        headers['Authorization'] = f'Bearer {token}'
        headers['Accept'] = 'application/vnd.github+json'
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers)) as r:
        return r.read()


def api(path):
    return json.loads(fetch(f'https://api.github.com/repos/{REPO}/{path}'))


def file_commits(ref):
    """The newest commits reachable from `ref` that touched the dictionary."""
    return api(f'commits?path={FILE}&sha={ref}&per_page=100')


def recorded():
    values = {}
    for line in open(UPSTREAM, encoding='utf-8'):
        key, sep, value = line.partition('=')
        if sep and not line.startswith('#'):
            values[key.strip()] = value.strip()
    for key in ('dictionary.commit', 'dictionary.sha256'):
        if key not in values:
            raise SystemExit(f'{UPSTREAM}: no {key} line')
    return values


def copyright_lines(text):
    """`# Copyright (c) …` header lines, without the `# `."""
    return [l[2:] for l in text.splitlines() if l.startswith('# Copyright (c) ')]


def release_name(text, sha, date):
    """What `dictionary.release` should say for the file at `sha`."""
    m = re.search(r'^# Release: (.+)$', text, re.M)
    if m and m.group(1).strip() != 'UNSET':
        return m.group(1).strip()
    tags = sorted((t for t in api('tags?per_page=100') if TAG.match(t['name'])),
                  key=lambda t: t['name'], reverse=True)
    for tag in tags:
        status = api(f'compare/{tag["name"]}...{sha}')['status']
        if status == 'identical':
            return tag['name']
        if status == 'ahead':
            before = {c['sha'] for c in file_commits(tag['name'])}
            n = sum(1 for c in file_commits(sha) if c['sha'] not in before)
            noun = 'commit' if n == 1 else 'commits'
            return (f'{sha[:7]} ({date} — {n} dictionary {noun} past release '
                    f'{tag["name"]})')
    return f'{sha[:7]} ({date} — no earlier release)'


def set_line(text, key, value):
    new, n = re.subn(rf'^{re.escape(key)} = .*$', lambda _: f'{key} = {value}', text,
                     count=1, flags=re.M)
    if n != 1:
        raise SystemExit(f'{UPSTREAM}: no {key} line')
    return new


def update_notice(path, old_sha, new_sha, date, old_lines, new_lines):
    """Moves what a NOTICE says about the vendored file; returns what it could not."""
    text = open(path, encoding='utf-8').read()
    todo = []
    text, n = re.subn(rf'{old_sha} \(\d{{4}}-\d{{2}}-\d{{2}}\)', f'{new_sha} ({date})', text)
    if n == 0:
        todo.append(f'`{os.path.relpath(path)}`: the dictionary commit was not found')
    if len(old_lines) == len(new_lines):
        for old, new in zip(old_lines, new_lines):
            # Whole lines only: the engine's notices share the holder's name
            # but end with the file they cover.
            text = text.replace(f'\n    {old}\n', f'\n    {new}\n')
    elif old_lines != new_lines:
        todo.append(f'`{os.path.relpath(path)}`: the copyright holders changed')
    open(path, 'w', encoding='utf-8').write(text)
    return todo


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--body', help='write a Markdown summary here')
    args = ap.parse_args()

    pinned = recorded()
    head = file_commits(BRANCH)[0]
    sha, date = head['sha'], head['commit']['committer']['date'][:10]
    raw = fetch(f'https://raw.githubusercontent.com/{REPO}/{sha}/{FILE}')
    digest = hashlib.sha256(raw).hexdigest()

    changed = digest != pinned['dictionary.sha256']
    out = {'changed': str(changed).lower(), 'commit': sha, 'short': sha[:7],
           'date': date, 'previous': pinned['dictionary.commit']}
    if not changed:
        print(f'up to date: {FILE} at {sha[:7]} matches UPSTREAM')
    else:
        text = raw.decode('utf-8')
        release = release_name(text, sha, date)
        out['release'] = release
        old_lines = copyright_lines(open(VENDORED, encoding='utf-8').read())
        new_lines = copyright_lines(text)

        with open(VENDORED, 'wb') as f:
            f.write(raw)
        upstream = open(UPSTREAM, encoding='utf-8').read()
        upstream = set_line(upstream, 'dictionary.commit', sha)
        upstream = set_line(upstream, 'dictionary.release', release)
        upstream = set_line(upstream, 'dictionary.sha256', digest)
        open(UPSTREAM, 'w', encoding='utf-8').write(upstream)
        todo = []
        for path in NOTICES:
            todo += update_notice(path, pinned['dictionary.commit'], sha, date,
                                  old_lines, new_lines)
        print(f'vendored {FILE} at {sha[:7]} ({date}): {release}')
        for item in todo:
            print(f'needs a person: {item}', file=sys.stderr)

        if args.body:
            previous = pinned['dictionary.commit']
            commits = file_commits(sha)
            seen = [c['sha'] for c in commits]
            if previous in seen:
                commits = commits[:seen.index(previous)]
            lines = [
                f'Upstream [{REPO}](https://github.com/{REPO}) changed `{FILE}` '
                f'on `{BRANCH}`. This vendors it at '
                f'[`{sha[:7]}`](https://github.com/{REPO}/commit/{sha}) ({date}), '
                f'up from `{previous[:7]}` — '
                f'[compare](https://github.com/{REPO}/compare/{previous}...{sha}).',
                '',
                f'`dictionary.release`: {release}',
                '',
                '### Upstream commits to the dictionary',
                '',
            ]
            for c in commits:
                subject = c['commit']['message'].splitlines()[0].strip()
                lines.append(f'- [`{c["sha"][:7]}`]({c["html_url"]}) {subject}')
            lines += [
                '',
                '### For the reviewer',
                '',
                '- [ ] Read the generated `ai.rs` diff against the upstream commits above.',
                '- [ ] Rewrite the NOTICE prose describing the vendored commit '
                '(both `NOTICE` and `crates/gs1-syntax/NOTICE`), and refine '
                '`dictionary.release` in `UPSTREAM` if it should say more '
                '(then rerun `cargo run -p gs1-syntax-gen`).',
                '- [ ] Decide whether `engine.commit` should move with it '
                '(test vectors and code lists — see README, *Regenerating*).',
                '- [ ] After merging, bump the `rev` in labelerkit-monorepo.',
            ]
            lines += [f'- [ ] {item}' for item in todo]
            with open(args.body, 'w', encoding='utf-8') as f:
                f.write('\n'.join(lines) + '\n')

    gh_out = os.environ.get('GITHUB_OUTPUT')
    if gh_out:
        with open(gh_out, 'a', encoding='utf-8') as f:
            for key, value in out.items():
                f.write(f'{key}={value}\n')


if __name__ == '__main__':
    main()
