# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=astrolabe
pkgver=0.1.0
pkgrel=1
pkgdesc="Desktop journal of your sport activities, read from your Garmin account"
arch=('x86_64' 'aarch64')
url="https://github.com/alarboulletmarin/astrolabe"
license=('MIT')
depends=('libxcb' 'libxkbcommon' 'libxkbcommon-x11' 'wayland' 'vulkan-icd-loader')
makedepends=('cargo' 'fontconfig' 'freetype2')
optdepends=('vulkan-driver: a Vulkan driver for your GPU is required to open the window'
            'gnome-keyring: where the Garmin key is kept (or kwallet, keepassxc); without one, an encrypted file is used')
# The binary is stripped at build time: a -debug package would be empty.
options=('!debug')
# Pinned by scripts/release.sh once the release tarball exists.
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release
}

package() {
    cd "$pkgname-$pkgver"
    make DESTDIR="$pkgdir" PREFIX=/usr install
}
