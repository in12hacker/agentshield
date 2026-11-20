#!/usr/bin/env python3
"""Fetch exact Debian source packages named by a real dpkg source inventory."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import lzma
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request

REPOSITORIES = [
    ('https://deb.debian.org/debian/', s, 'main')
    for s in ['trixie', 'trixie-updates', 'trixie-backports', 'bookworm', 'bookworm-updates']
] + [('https://deb.debian.org/debian-security/', s, 'main')
     for s in ['trixie-security', 'bookworm-security']] + [
    ('https://ports.ubuntu.com/ubuntu-ports/', s, component)
    for s in ['noble', 'noble-updates'] for component in ['main', 'universe']
]


def fetch(url, target, expected=None):
    if target.exists():
        data = target.read_bytes()
    else:
        with urllib.request.urlopen(url, timeout=90) as response:
            data = response.read()
    actual = hashlib.sha256(data).hexdigest()
    if expected is not None and actual != expected:
        raise ValueError('source checksum mismatch: ' + url)
    if not target.exists():
        temporary = target.with_name(target.name + '.part')
        temporary.write_bytes(data)
        temporary.replace(target)
    return data


def records(raw):
    for paragraph in raw.split('\n\n'):
        fields = {}
        current = None
        for line in paragraph.splitlines():
            if line.startswith((' ', '\t')) and current is not None:
                fields[current] += '\n' + line
            elif ':' in line and not line.startswith((' ', '\t')):
                current, value = line.split(':', 1)
                fields[current] = value.lstrip()
        if fields:
            yield fields


def indexes(cache):
    cache.mkdir(parents=True, exist_ok=True)

    def one(repository):
        base, suite, component = repository
        url = base + 'dists/' + suite + '/' + component + '/source/Sources.xz'
        path = cache / (suite + '-' + component + '-Sources.xz')
        data = fetch(url, path)
        return [(base, r) for r in records(lzma.decompress(data).decode())]

    with ThreadPoolExecutor(max_workers=4) as pool:
        return [item for batch in pool.map(one, REPOSITORIES) for item in batch]


def collect(inventory, cache, output):
    wanted = {tuple(row.split('\t')) for row in inventory.read_text().splitlines() if row}
    if any(len(row) != 2 for row in wanted):
        raise ValueError('expected source-package<TAB>source-version inventory')
    selected = {}
    for base, record in indexes(cache):
        key = record.get('Package'), record.get('Version')
        if key in wanted:
            selected[key] = base, record
    missing = sorted(wanted - selected.keys())
    output.mkdir(parents=True, exist_ok=True)

    def one(entry):
        (name, version), (base, record) = entry
        folder = output / (name + '_' + version.replace(':', '%3A'))
        folder.mkdir(exist_ok=True)
        files = []
        for row in record['Checksums-Sha256'].splitlines():
            if not row.strip():
                continue
            digest, size, filename = row.split()
            if Path(filename).name != filename:
                raise ValueError('non-basename source filename')
            url = base + record['Directory'] + '/' + urllib.parse.quote(filename)
            target = folder / filename
            data = fetch(url, target, digest)
            if len(data) != int(size):
                raise ValueError('source size mismatch: ' + filename)
            files.append({'file': str(target.relative_to(output)), 'sha256': digest, 'url': url})
        return {'source': name, 'version': version, 'files': files}

    with ThreadPoolExecutor(max_workers=4) as pool:
        completed = list(pool.map(one, sorted(selected.items())))
    report = {'packages': completed, 'not_in_current_indexes': missing}
    (output / 'source-manifest.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'downloaded_source_packages': len(completed), 'missing': missing}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--indexes-only', action='store_true')
    parser.add_argument('cache', type=Path)
    parser.add_argument('inventory', type=Path, nargs='?')
    parser.add_argument('output', type=Path, nargs='?')
    args = parser.parse_args()
    if args.indexes_only:
        print('Indexed source records:', len(indexes(args.cache)))
    elif args.inventory is not None and args.output is not None:
        collect(args.inventory, args.cache, args.output)
    else:
        parser.error('inventory and output are required for source collection')
