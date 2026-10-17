#!/usr/bin/env python3
"""Resolve stable vendor DEBs through a pinned-key, signed APT metadata chain."""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import gzip
import hashlib
import json
import lzma
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile
from urllib.parse import urljoin, urlparse
from urllib.request import Request, urlopen

VENDORS = (
    {'name': 'google-chrome', 'deb_name': 'google-chrome-stable',
     'base': 'https://dl.google.com/linux/chrome/deb/',
     'key_url': 'https://dl.google.com/linux/linux_signing_key.pub',
     'fingerprints': ('EB4C1BFD4F042F6DDDCCEC917721F63BD38B4796',)},
    {'name': 'brave-bin', 'deb_name': 'brave-browser',
     'base': 'https://brave-browser-apt-release.s3.brave.com/',
     'key_url': 'https://brave-browser-apt-release.s3.brave.com/brave-browser-archive-keyring.gpg',
     'fingerprints': ('DBF1A116C220B8C7164F98230686B78420038257',
                      '47D32A74E9A9E013A4B4926C68D513D36A73CD96',
                      'B2A3DCA350E67256740DF904DE4EC67BE4B0DCA0')},
)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def safe_relative(value):
    path = PurePosixPath(value)
    if not value or path.is_absolute() or '..' in path.parts or '\\' in value or ':' in value:
        raise ValueError(f'Unsafe vendor metadata path: {value!r}')
    return value


def download(url, *, max_bytes=10 * 1024**2):
    parsed = urlparse(url)
    if parsed.scheme != 'https' or parsed.hostname not in {
        'dl.google.com', 'brave-browser-apt-release.s3.brave.com'}:
        raise ValueError(f'Unapproved vendor download URL: {url}')
    with urlopen(Request(url, headers={'User-Agent': '02OS-browser-package-builder/1'}), timeout=90) as response:
        final = urlparse(response.url)
        if final.scheme != 'https' or final.hostname != parsed.hostname:
            raise ValueError(f'Unexpected download redirect: {response.url}')
        data = response.read(max_bytes + 1)
    if len(data) > max_bytes:
        raise ValueError(f'Vendor response exceeds size limit: {url}')
    return data


def parse_control(text):
    paragraphs = []
    current = {}
    key = None
    for line in text.splitlines() + ['']:
        if not line:
            if current:
                paragraphs.append(current)
            current, key = {}, None
        elif line[0].isspace():
            if key is None:
                raise ValueError('Continuation without a control field')
            current[key] += '\n' + line[1:]
        else:
            key, separator, value = line.partition(':')
            if not separator or not key or key in current:
                raise ValueError(f'Malformed/duplicate control field: {line}')
            current[key] = value.lstrip()
    return paragraphs


def select_index(release):
    hashes = {}
    for line in release.get('SHA256', '').splitlines():
        fields = line.split()
        if len(fields) != 3:
            continue
        digest, size, name = fields
        if not re.fullmatch('[a-fA-F0-9]{64}', digest) or not size.isdigit():
            raise ValueError('Malformed signed SHA256 entry')
        hashes[safe_relative(name)] = (digest.lower(), int(size))
    for name in ('main/binary-amd64/Packages.xz', 'main/binary-amd64/Packages.gz',
                 'main/binary-amd64/Packages'):
        if name in hashes:
            return name, *hashes[name]
    raise ValueError('Signed Release has no amd64 package index')


def package_version(version):
    """Preserve a vendor rebuild revision in the native pacman version."""
    upstream, separator, revision = version.rpartition("-")
    return (upstream, str(int(revision))) if separator else (version, "1")


def select_package(paragraphs, name):
    packages = [p for p in paragraphs if p.get('Package') == name and p.get('Architecture') == 'amd64']
    if not packages:
        raise ValueError(f'Stable repository has no amd64 {name}')
    # These vendor versions are numeric dotted releases plus numeric pkgrel.
    for p in packages:
        if not re.fullmatch(r'\d+(?:\.\d+)+(?:-\d+)?', p.get('Version', '')):
            raise ValueError(f'Unexpected stable version: {p.get("Version")}')
    package = max(packages, key=lambda p: tuple(map(int, re.split(r'[.-]', p['Version']))))
    safe_relative(package['Filename'])
    if not re.fullmatch('[a-fA-F0-9]{64}', package.get('SHA256', '')):
        raise ValueError('Package has no SHA256 in verified index')
    if not package.get('Size', '').isdigit():
        raise ValueError('Package has no valid size in verified index')
    return package


def verified_release(vendor, output):
    with tempfile.TemporaryDirectory(prefix='02os-browser-gpg-') as temporary:
        home = Path(temporary)
        home.chmod(0o700)
        key_file = home / 'downloaded-key'
        key_file.write_bytes(download(vendor['key_url']))
        common = ['gpg', '--batch', '--homedir', str(home)]
        subprocess.run([*common, '--import', str(key_file)], check=True, capture_output=True)
        listing = subprocess.check_output([*common, '--with-colons', '--list-keys'], text=True)
        primary = set()
        pending = False
        for line in listing.splitlines():
            fields = line.split(':')
            if fields[0] == 'pub':
                pending = True
            elif fields[0] == 'fpr' and pending:
                primary.add(fields[9]); pending = False
        trusted = sorted(primary.intersection(vendor['fingerprints']))
        if not trusted:
            raise ValueError(f'{vendor["name"]}: vendor key has no pinned primary fingerprint')
        keyring = home / 'trusted.gpg'
        keyring.write_bytes(subprocess.check_output([*common, '--export', *trusted]))
        root = vendor['base'] + 'dists/stable/'
        signed = home / 'InRelease'
        try:
            signed.write_bytes(download(root + 'InRelease'))
        except Exception as exc:
            # Some Google repository mirrors expose detached Release.gpg only.
            from urllib.error import HTTPError
            if not isinstance(exc, HTTPError) or exc.code != 404:
                raise
            release = home / 'Release'
            release.write_bytes(download(root + 'Release'))
            signature = home / 'Release.gpg'
            signature.write_bytes(download(root + 'Release.gpg'))
            subprocess.run(['gpgv', '--homedir', str(home), '--keyring', str(keyring),
                            str(signature), str(release)], check=True, capture_output=True)
            content = release.read_bytes()
            signed_hash = sha256(signature.read_bytes())
        else:
            release = home / 'Release'
            subprocess.run(['gpgv', '--homedir', str(home), '--keyring', str(keyring),
                            '--output', str(release), str(signed)], check=True, capture_output=True)
            content = release.read_bytes()
            signed_hash = sha256(signed.read_bytes())
        (output / 'verified-Release').write_bytes(content)
        return content.decode('utf-8'), trusted, signed_hash


def resolve(vendor, destination):
    destination.mkdir(parents=True, exist_ok=False)
    release_text, fingerprints, signed_hash = verified_release(vendor, destination)
    release_records = parse_control(release_text)
    if len(release_records) != 1:
        raise ValueError('Expected one verified Release record')
    release = release_records[0]
    # Reject expired/future signed indexes if the vendor supplies these fields.
    from email.utils import parsedate_to_datetime
    now = datetime.now(timezone.utc)
    if release.get('Date') and (parsedate_to_datetime(release['Date']) - now).total_seconds() > 86400:
        raise ValueError('Signed repository metadata is dated in the future')
    if release.get('Valid-Until') and parsedate_to_datetime(release['Valid-Until']) < now:
        raise ValueError('Signed repository metadata has expired')
    index_name, index_hash, index_size = select_index(release)
    packed = download(vendor['base'] + 'dists/stable/' + index_name)
    if len(packed) != index_size or sha256(packed) != index_hash:
        raise ValueError('Vendor package index does not match signed Release')
    data = lzma.decompress(packed) if index_name.endswith('.xz') else gzip.decompress(packed) if index_name.endswith('.gz') else packed
    package = select_package(parse_control(data.decode('utf-8')), vendor['deb_name'])
    package_url = urljoin(vendor['base'], package['Filename'])
    content = download(package_url, max_bytes=512 * 1024**2)
    if len(content) != int(package['Size']) or sha256(content) != package['SHA256'].lower():
        raise ValueError('Vendor binary does not match the verified package index')
    (destination / 'browser.deb').write_bytes(content)
    version, revision = package_version(package['Version'])
    record = {'name': vendor['name'], 'vendor_package': vendor['deb_name'],
              'vendor_version': package['Version'], 'pkgver': version, 'pkgrel': revision,
              'source_url': package_url, 'source_sha256': sha256(content),
              'repository': vendor['base'], 'trusted_primary_fingerprints': fingerprints,
              'signed_metadata_sha256': signed_hash,
              'verified_release_sha256': sha256(release_text.encode()),
              'release_date': release.get('Date'), 'index_filename': index_name,
              'index_sha256': index_hash}
    (destination / 'source.json').write_text(json.dumps(record, indent=2) + '\n')
    print(f'{vendor["name"]}: verified stable {package["Version"]}', flush=True)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    sources = [resolve(vendor, args.output / vendor['name']) for vendor in VENDORS]
    (args.output / 'sources.json').write_text(json.dumps(sources, indent=2) + '\n')


if __name__ == '__main__':
    main()
