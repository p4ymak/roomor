# Maintainer: Roman Chumak <p4ymak@yandex.ru>
pkgname=roomor
pkgver=0.3.8
pkgrel=1
makedepends=('rust' 'cargo')
arch=('i686' 'x86_64' 'armv6h' 'armv7h')
pkgdesc="Minimalistic offline chat over local network."
license=('MIT')

build() {
    return 0
}

package() {
    cd $srcdir
    cargo install --root="$pkgdir" --git=https://github.com/p4ymak/roomor
}
