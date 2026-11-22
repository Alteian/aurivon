#!/bin/bash
set -e

echo "Installing dependencies for Aurivon..."

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
            mpv \
            ffmpeg \
            libxkbcommon-dev \
            libwayland-dev \
            libvulkan-dev \
            libfontconfig1-dev \
            libfreetype6-dev
        ;;
    "Fedora")
        echo "Detected Fedora"
        sudo dnf install -y \
            gcc gcc-c++ \
            pkg-config \
            openssl-devel \
            mpv \
            libxkbcommon-devel \
            wayland-devel \
            vulkan-loader-devel \
            fontconfig-devel \
            freetype-devel
        ;;
    "Arch Linux")
        echo "Detected Arch Linux"
        sudo pacman -S --needed \
            base-devel \
            pkgconf \
            openssl \
            mpv \
            libxkbcommon \
            wayland \
            vulkan-icd-loader \
            fontconfig \
            freetype2
        ;;
    *)
        echo "Unsupported OS: $OS"
        echo "Please install dependencies manually."
        exit 1
        ;;
esac

echo "Dependencies installed successfully!"
