# Security

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Use GitHub's private reporting instead:
[Report a vulnerability](https://github.com/kyzmapiratov/Menagerie/security/advisories/new). Say what you found, how to
reproduce it and what an attacker could do with it. You will get an answer within a week, and credit in the release notes
if you want it.

## What is in scope

Menagerie runs on your computer, as you, and does these things worth being careful about:

- it starts programs (`shimejictl`, `shimeji-overlayd`, your file manager, `gio`);
- it unpacks archives you download and writes files into the engine's folders;
- it fetches two websites (shimejis.xyz and cachomon.com) and shows their pictures;
- optionally it writes a configuration file for niri.

Reports about any of these are welcome: path traversal in an archive, a command run with unescaped input, a way for a web
page or a catalog entry to make the app do something it should not, or the app deleting a file it should not have touched.
The web view runs with a strict Content Security Policy and a scoped asset protocol; see the security notes in
[docs/architecture.md](../docs/architecture.md#security-notes).

Out of scope: problems in `wl_shimeji` itself (report them [upstream](https://github.com/CluelessCatBurger/wl_shimeji)),
and in the catalogs' websites.

## Supported versions

Only the latest release gets fixes.
