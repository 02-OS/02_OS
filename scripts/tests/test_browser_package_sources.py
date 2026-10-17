"""Exercise browser source authentication and tampering rejection offline."""
from datetime import datetime, timezone
from email.utils import format_datetime
import gzip
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('browser_sources', ROOT / 'scripts/browser-packages/resolve_sources.py')
sources = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sources)


@unittest.skipUnless(shutil.which('gpg') and shutil.which('gpgv'), 'GnuPG is required')
class BrowserSourceVerificationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix='02os-browser-signature-test-')
        cls.home = Path(cls.temporary.name)
        cls.home.chmod(0o700)
        cls.gpg = ['gpg', '--batch', '--homedir', str(cls.home)]
        subprocess.run([*cls.gpg, '--pinentry-mode', 'loopback', '--passphrase', '',
                        '--quick-generate-key', '02OS test <browser-test@example.invalid>',
                        'ed25519', 'sign', '1d'], check=True, capture_output=True)
        listing = subprocess.check_output([*cls.gpg, '--with-colons', '--list-keys'], text=True)
        cls.fingerprint = next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
        cls.public = subprocess.check_output([*cls.gpg, '--export', cls.fingerprint])

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['gpgconf', '--homedir', str(cls.home), '--kill', 'all'], check=False, capture_output=True)
        cls.temporary.cleanup()

    def fixture(self):
        payload = b'An authenticated synthetic package, never executed.'
        index = ('Package: test-browser\nVersion: 155.0.8059.79-1\nArchitecture: amd64\n'
                 'Filename: pool/main/t/test-browser.deb\n'
                 f'Size: {len(payload)}\nSHA256: {sources.sha256(payload)}\n\n').encode()
        packed = gzip.compress(index)
        release = (f'Date: {format_datetime(datetime.now(timezone.utc))}\n'
                   f'SHA256:\n {sources.sha256(packed)} {len(packed)} main/binary-amd64/Packages.gz\n').encode()
        signed = subprocess.check_output([*self.gpg, '--armor', '--clearsign'], input=release)
        vendor = {'name': 'test-browser', 'deb_name': 'test-browser', 'base': 'https://dl.google.com/test/',
                  'key_url': 'https://dl.google.com/test/key', 'fingerprints': (self.fingerprint,)}
        responses = {vendor['key_url']: self.public, vendor['base'] + 'dists/stable/InRelease': signed,
                     vendor['base'] + 'dists/stable/main/binary-amd64/Packages.gz': packed,
                     vendor['base'] + 'pool/main/t/test-browser.deb': payload}
        return vendor, responses

    def test_signed_metadata_authenticates_exact_downloaded_binary(self):
        vendor, responses = self.fixture()
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sources, 'download', side_effect=lambda url, **kw: responses[url]):
            destination = Path(temporary) / 'resolved'
            record = sources.resolve(vendor, destination)
            self.assertEqual(record['vendor_version'], '155.0.8059.79-1')
            self.assertEqual(record['source_sha256'], sources.sha256((destination / 'browser.deb').read_bytes()))
            self.assertEqual(record['trusted_primary_fingerprints'], [self.fingerprint])

    def test_tampered_index_fails_before_binary_download(self):
        vendor, responses = self.fixture()
        index_url = vendor['base'] + 'dists/stable/main/binary-amd64/Packages.gz'
        responses[index_url] += b'tampered'
        requested = []
        def download(url, **kwargs):
            requested.append(url)
            return responses[url]
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sources, 'download', side_effect=download):
            with self.assertRaisesRegex(ValueError, 'index does not match'):
                sources.resolve(vendor, Path(temporary) / 'resolved')
        self.assertFalse(any('/pool/' in url for url in requested))

    def test_tampered_binary_is_not_written(self):
        vendor, responses = self.fixture()
        responses[vendor['base'] + 'pool/main/t/test-browser.deb'] += b'tampered'
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sources, 'download', side_effect=lambda url, **kw: responses[url]):
            destination = Path(temporary) / 'resolved'
            with self.assertRaisesRegex(ValueError, 'binary does not match'):
                sources.resolve(vendor, destination)
            self.assertFalse((destination / 'browser.deb').exists())

    def test_unpinned_signing_key_is_rejected(self):
        vendor, responses = self.fixture()
        vendor['fingerprints'] = ('0' * 40,)
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sources, 'download', side_effect=lambda url, **kw: responses[url]):
            with self.assertRaisesRegex(ValueError, 'no pinned primary'):
                sources.resolve(vendor, Path(temporary) / 'resolved')

    def test_tampered_signed_release_is_rejected(self):
        vendor, responses = self.fixture()
        url = vendor['base'] + 'dists/stable/InRelease'
        responses[url] = responses[url].replace(b'SHA256:', b'SHA255:')
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(sources, 'download', side_effect=lambda url, **kw: responses[url]):
            with self.assertRaises(subprocess.CalledProcessError):
                sources.resolve(vendor, Path(temporary) / 'resolved')


class BrowserSourceParsingTests(unittest.TestCase):
    def test_vendor_rebuild_revision_is_not_collapsed_for_pacman_needed(self):
        self.assertEqual(sources.package_version("155.0.8059.79-2"), ("155.0.8059.79", "2"))
        self.assertEqual(sources.package_version("1.97.57"), ("1.97.57", "1"))

    def test_latest_numeric_release_not_lexicographic_order(self):
        packages = [dict(Package='browser', Architecture='amd64', Version=version,
                         Filename='pool/browser.deb', Size='4', SHA256='a' * 64)
                    for version in ('1.9.99-1', '1.10.1-1')]
        self.assertEqual(sources.select_package(packages, 'browser')['Version'], '1.10.1-1')

    def test_repository_cannot_redirect_binary_to_other_origin_or_path(self):
        for path in ('https://example.org/payload', '../payload.deb', '/etc/passwd', 'pool/../../payload'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                sources.safe_relative(path)

    def test_unsigned_index_without_strong_hash_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'no amd64 package index'):
            sources.select_index({'MD5Sum': '0' * 32 + ' 123 main/binary-amd64/Packages'})


if __name__ == '__main__':
    unittest.main()
