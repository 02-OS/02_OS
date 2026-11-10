# bootc scaffold

`bootc/Containerfile` is a scaffold. `./build.sh` does not call it, and it is not part of the ISO profile. The image build remains `./build.sh`, which stages the agent runtime, checks those binaries, and runs `mkarchiso`.

This file does not install packages and does not claim a boot time. No boot-time number is recorded here because this tree has no measurement for one.

GitHub-hosted runners cannot produce this image. The disk, the Arch mirror, and `mkarchiso` are not available there. `.github/workflows/image.yml` is `workflow_dispatch` only. A pull request does not start it, and the workflow does not run `build.sh`.

Build an ISO on a machine with at least 40 GiB free. Do not delete the retained images `2026.09.26`, `2026.10.01`, or `2026.10.06`.
