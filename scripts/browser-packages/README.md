# Native stable browser packages

`../build-browser-packages.sh` resolves the current x86_64 stable release from
Google's and Brave's signed APT repositories, verifies the complete metadata
chain, and wraps each unchanged vendor binary in a pacman package. It executes
only these checked-in PKGBUILDs, using unprivileged `makepkg` in an Arch container.
It does not install APT, execute Debian maintainer scripts, or install browsers
on the host.

Outputs in `.cache/browser-repo` are `google-chrome-*.pkg.tar.zst`,
`brave-bin-*.pkg.tar.zst`, `02os-browsers.db`, its archive, and `manifest.json`.
Mount that directory as `/browser-repo` for an image-build-only pacman repository:

```
[02os-browsers]
SigLevel = Optional TrustAll
Server = file:///browser-repo
```

That trust exception applies only to these locally built packages. Their vendor
payloads have already been verified; it must not replace the global Arch
signature policy. Preserve `manifest.json` with image build evidence. The cached
packages must also be installed into the installed OS using pacman; merely
copying their desktop entries or pinning them is insufficient.

The upstream Debian rebuild revision is preserved as the pacman `pkgrel`, so
`--needed` does not skip a vendor rebuild. Arch build dependencies are cached
under `.cache/browser-build-pacman` to avoid repeated downloads.

Normal invocation refreshes signed stable metadata every time and fails closed
on network, signature, hash, or package build failures. `--output DIR` selects a
host output directory. The fixed vendor key fingerprints are reviewed source;
key rotation requires an intentional update rather than trusting a key returned
by an unverified repository. `--resolve-only` verifies and downloads source
packages without executing PKGBUILDs (still in the disposable container).

The package names are `google-chrome` and `brave-bin`; their desktop IDs are
`google-chrome.desktop` and `brave-browser.desktop`. Standard Chromium sandbox
binaries retain mode 4755, and launchers do not disable sandboxing. Debian APT
configuration, cron jobs, and maintainer scripts are omitted.

These local packages do not create an online pacman update service. Re-run this
builder and install the resulting packages to update an existing system, or
publish a separately managed signed pacman repository. The build will always
resolve fresh stable metadata; an existing installed image is a fixed snapshot.

During image customization, `02os-install-browsers --verify-only` checks the
pacstrap-installed versions without another package transaction, mirrors, or
keyring initialization. The installer copies the helper and manifest with the
desktop profile, then checks the target versions after desktop installation.
Only a target needing those versions receives the archive payload. A successful
install removes manifest-listed, hash-matching staged archives and repository
databases; the small manifest remains. Failed installs retain payload for
diagnosis, and repeated hooks do not restore spent archives.

Primary vendor references:
- https://www.google.com/linuxrepositories/
- https://brave.com/linux/
- https://brave.com/signing-keys/
- https://github.com/brave/brave-browser/wiki/(Re)packaging
