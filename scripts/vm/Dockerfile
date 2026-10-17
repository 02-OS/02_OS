FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        qemu-system-x86 qemu-system-gui qemu-utils ovmf libgl1-mesa-dri libegl1 \
    && rm -rf /var/lib/apt/lists/*
