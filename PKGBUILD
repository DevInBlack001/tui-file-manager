# Maintainer: DevInBlack001 <DevInBlack001@users.noreply.github.com>
pkgname=tui-file-manager
pkgver=0.3.8
pkgrel=1
pkgdesc="Keyboard-driven TUI file manager for Arch/Omarchy (binary: fim)"
arch=('x86_64')
url="https://github.com/DevInBlack001/tui-file-manager"
license=('MIT')
depends=('gcc-libs')
makedepends=('cargo')
optdepends=(
    'chafa: image previews'
    'ffmpegthumbnailer: video thumbnails'
    'bat: syntax-highlighted text previews'
    'trash-cli: trash (soft-delete) support'
    'poppler: pdftotext, for PDF first-page previews'
    'ttf-jetbrains-mono-nerd: directory/file type icons in the file list'
    'python-pywal: live theme colors on non-Omarchy systems'
    'gvfs: mount phones over MTP into the DEVICES sidebar section'
    'gvfs-mtp: mount phones over MTP into the DEVICES sidebar section'
    'git: required by the separate ftctl/filetransferd plugin installer for copy/move support'
)
provides=('fim')
conflicts=('fim')
source=("$pkgname-$pkgver.tar.gz::https://github.com/DevInBlack001/tui-file-manager/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('cd83bbd8ceae183d27342b98bcdc0f20f8d84c8e2b89b9d1cfe75feebc1ca584')

prepare() {
    cd "$pkgname-$pkgver"
    cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
    cd "$pkgname-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release
}

check() {
    cd "$pkgname-$pkgver"
    cargo test --frozen --release
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/fim" "$pkgdir/usr/bin/fim"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 "CHANGELOG.md" "$pkgdir/usr/share/doc/$pkgname/CHANGELOG.md"

    install -Dm644 /dev/stdin "$pkgdir/usr/share/applications/fim.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=fim
GenericName=File Manager
Comment=Keyboard-driven TUI file manager for Arch/Omarchy
TryExec=fim
Exec=fim
Terminal=true
Icon=system-file-manager
Categories=System;FileManager;FileTools;ConsoleOnly;
Keywords=files;filemanager;terminal;tui;
StartupNotify=false
EOF
}
