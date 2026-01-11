#!/bin/bash
set -e

echo "Installing dependencies for RustMediaHub..."

if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$NAME
elif type lsb_release >/dev/null 2>&1; then
    OS=$(lsb_release -si)
else
    OS=$(uname -s)
fi

case "$OS" in
    "Ubuntu"|"Debian"|"Pop!_OS")
        echo "Detected Debian/Ubuntu-based system"
        sudo apt-get update
        sudo apt-get install -y \
            build-essential \
            pkg-config \
            libssl-dev \
            libsqlite3-dev \
            libmpv-dev \
            mpv \
            libgtk-3-dev \
            libxkbcommon-dev \
            libwayland-dev \
            libvulkan-dev
        ;;
    "Fedora")
        echo "Detected Fedora"
        sudo dnf install -y \
            gcc gcc-c++ \
            pkg-config \
            openssl-devel \
            sqlite-devel \
            mpv-libs-devel \
            mpv \
            gtk3-devel \
            libxkbcommon-devel \
            wayland-devel \
            vulkan-loader-devel
        ;;
    "Arch Linux")
        echo "Detected Arch Linux"
        sudo pacman -S --needed \
            base-devel \
            pkgconf \
            openssl \
            sqlite \
            mpv \
            gtk3 \
            libxkbcommon \
            wayland \
            vulkan-icd-loader
        ;;
    *)
        echo "Unsupported OS: $OS"
        echo "Please install dependencies manually."
        exit 1
        ;;
esac

echo "Dependencies installed successfully!"
