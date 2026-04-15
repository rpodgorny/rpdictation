# Maintainer: Radek Podgorny <radek@podgorny.cz>
pkgname=rpdictation-git
provides=('rpdictation')
conflicts=('rpdictation')
pkgver=r68.216ff32
pkgrel=1
pkgdesc="Radek Podgorny's speech-to-text dictation tool"
arch=('x86_64')
url="https://github.com/rpodgorny/rpdictation"
license=('GPL-3.0-or-later')
depends=('alsa-lib')
makedepends=('git' 'cargo')
optdepends=('wtype: text insertion on Wayland'
            'ydotool: text insertion via uinput')
source=("$pkgname::git+https://github.com/rpodgorny/rpdictation")
md5sums=('SKIP')

pkgver() {
	cd "$srcdir/$pkgname"
	printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

build() {
	cd "$srcdir/$pkgname"
	cargo build --release --locked
}

package() {
	cd "$srcdir/$pkgname"
	install -D -m 0755 -t $pkgdir/usr/bin/ target/release/rpdictation
}
