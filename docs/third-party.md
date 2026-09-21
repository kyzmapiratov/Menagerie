# Third-party code and assets

Menagerie itself is GPL-2.0 licensed (see [LICENSE](../LICENSE)). This file lists
everything else that ships with it or that it depends on.

## Bundled in this repository

### Shimeji-ee default configuration files

| | |
|---|---|
| Files | `src-tauri/assets/default-actions.xml`, `src-tauri/assets/default-behaviors.xml` |
| Upstream | <https://github.com/TigerHix/shimeji-ee> (`conf/`) |
| Stated license | New BSD (3-clause) |
| Origin | Shimeji was originally created by Yuki Yamada of Group Finity; Shimeji-ee is the English branch of that project |

**Why they are here.** shimejis.xyz distributes sprite sheets only, and
`wl_shimeji` refuses a package that has no `actions.xml` / `behaviors.xml`. The
app therefore adds these default files to every package it builds from
shimejis.xyz sprites.

**A note on the copyright line.** Upstream states "The Shimeji-ee source is
under the New BSD license" in its readme but does not ship a `LICENSE` file with
a copyright line, and the various forks word it differently. The terms below are
the standard 3-clause BSD text, reproduced here so that the license travels with
the files. If you redistribute this project and need a stricter paper trail,
confirm the wording with the Shimeji-ee maintainers.

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Required at runtime, not bundled

### wl_shimeji

| | |
|---|---|
| Project | <https://github.com/CluelessCatBurger/wl_shimeji> |
| License | GPL-2.0 |
| Used as | A separate program. The app runs the `shimejictl` command and reads its output; no `wl_shimeji` code is linked into this binary. |

`wl_shimeji` is the engine: it draws the characters on your desktop. Menagerie
only drives it. You install it yourself (see [install.md](install.md)), or `install.sh` does, and it stays
under its own license.

## Icon and logo

`packaging/menagerie.svg` (and the PNG sizes rendered from it, and `docs/img/logo.svg`) is original artwork made for this
project, under the same GPL-2.0 license as the code.

## Build dependencies

The Rust crates and the Tauri framework are pulled in by Cargo and are not
redistributed in this repository. Tauri is dual-licensed Apache-2.0 / MIT. About 600 crates are linked
into the binary; checked against `Cargo.lock` (`cargo metadata`), all of them are under permissive or
weak copyleft licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, Unlicense, BSL-1.0, MPL-2.0, LGPL),
which are fully compatible with Menagerie's GPL-2.0 license. Re-check after adding a dependency:
`cargo metadata --format-version 1 --locked` lists every crate's `license`.

The front end is plain files with no bundled JavaScript library and no web font, so no npm package ends up in what is shipped
(`package.json` holds the Tauri command-line tool and API packages for building).

**System libraries.** The app links GTK 3 and WebKitGTK 4.1 (LGPL-2.1+) *dynamically*, and `libayatana-appindicator` (LGPL / GPL
dual) is opened at run time for the tray icon; none of them is copied into this repository or the packages. A binary release
that includes the compiled crates carries their copyright notices in the crates' own source; a license-text bundle for the
release can be generated with `cargo about` if a distribution asks for one.

## Character art

No character sprites are stored in this repository. They are downloaded by the
user, at the user's request, from the catalogs described in the [README](../README.md). The
copyright in each character belongs to its creator or rights holder.
