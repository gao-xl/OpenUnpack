# Third-party components

OpenUnpack application source is MIT licensed. This does not relicense its
dependencies or Microsoft WebView2. Keep `LICENSE`, this file, the complete
`worker/` directory and `release-materials/` with binary redistribution.

`scripts/collect-release-materials.ps1` collects dependency versions, original
license/notice texts, exact Windows Rust source archives and 7-Zip source.
`release-materials/manifest.json` contains the versions, material provenance and
SHA-256 values. Collection includes build/test dependencies conservatively;
their inclusion does not mean every component executes in the installed app.
Do not redistribute a release if collection failed or materials are missing.

The customized `apps/desktop-host/installer.nsi` derives from the Tauri CLI
2.12.1 NSIS template, licensed under Apache-2.0 OR MIT by Tauri Programme within
The Commons Conservancy. Its upstream source and SHA-256 are recorded in the
file header; its MIT license is supplied in `installer-LICENSE-MIT.txt`.
OpenUnpack changes the default program directory and protects the
persistent data directory. Source: https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi

## 7-Zip 26.04

Copyright (C) 1999–2026 Igor Pavlov. The separately replaceable x64 `7z.dll` is
official and unmodified; no source-program binary or database is used.

- Project: https://www.7-zip.org/
- License: https://www.7-zip.org/license.txt
- Exact binary/source release: https://github.com/ip7z/7zip/releases/tag/26.04
- Corresponding source in the distribution: `release-materials/7zip/7z2604-src.7z`

Most code uses GNU LGPL 2.1 or later; some code carries the unRAR restriction,
BSD 2-clause or BSD 3-clause terms. Preserve the full upstream license and LGPL
text in `release-materials/7zip/`. The unRAR restriction prohibits using that
decoder code to develop an RAR-compatible archiver. OpenUnpack only extracts
archives and does not supply a separate unRAR executable.

The DLL is dynamically loaded. Users may replace it with a compatible build,
including a modified library for their own use; redistribution must retain
the applicable license/source materials. The source tree does not commit
downloaded engine binaries. The official MSI and source archive are checked
against pinned SHA-256 values by the setup/release scripts.

## .NET Runtime 10.0.13

Copyright .NET Foundation and Contributors. The self-contained Worker includes
Microsoft .NET Runtime, primarily MIT licensed with component-specific notices.
The SDK publish output does not reliably include those notices automatically;
the release collector supplies matching versioned upstream texts:

- `release-materials/dotnet/LICENSE.TXT`
- `release-materials/dotnet/THIRD-PARTY-NOTICES.TXT`
- https://github.com/dotnet/runtime/tree/v10.0.13

Reviewed runtime versions and matching notice checksums are maintained together
in `scripts/runtime-policy.json`. Preparation, collection and the security audit
check the published runtime identity; online security auditing also checks for
later official security releases. A clean NuGet report does not cover the
self-contained runtime.

The modern application does not package WPF. The preserved early WPF prototype
is source only and uses Microsoft framework components under their own terms:
https://github.com/dotnet/wpf/blob/main/LICENSE.TXT

## Tauri, Rust and SQLite

Tauri and its ecosystem generally offer MIT or Apache-2.0 choices; dependencies
retain individual licenses. Rust dependency notices and exact `.crate` source
archives are in `release-materials/rust/`, with the platform graph derived from
Cargo.lock. Some build dependencies use MPL-2.0; keep their corresponding source
archives and notices. SQLite itself is public domain, while Rust wrappers have
their own licenses. The collected `libsqlite3-sys` archive contains the bundled
SQLite source and notices.

- https://github.com/tauri-apps/tauri
- https://www.sqlite.org/copyright.html
- Dependency versions and licenses: `release-materials/manifest.json`

## React, Ant Design and frontend dependencies

React, Ant Design and their icons principally use MIT. Vite, TypeScript, Tauri
JavaScript APIs and all transitive installed dependencies retain their own
licenses. Original notice texts and per-package versions are collected under
`release-materials/npm/`; package-lock.json records dependency integrity.

- https://github.com/facebook/react/blob/main/LICENSE
- https://github.com/ant-design/ant-design/blob/master/LICENSE
- https://github.com/ant-design/ant-design-icons/blob/master/LICENSE

## Axum, Utoipa and Swagger UI

The local API uses Axum and Utoipa under their upstream MIT and/or Apache-2.0
terms. Official Swagger UI 5.32.6 static assets are embedded directly and retain
their Apache-2.0 license and NOTICE.
The original texts and exact archive provenance are kept in
`assets/legal/swagger-ui/` in source and `release-materials/swagger-ui/` in
binary distributions. Cargo component licenses remain under `release-materials/rust/`.

- https://github.com/tokio-rs/axum
- https://github.com/juhaku/utoipa
- https://github.com/swagger-api/swagger-ui

## Microsoft Edge WebView2 and installer tools

WebView2 is a separate Microsoft runtime and is governed by Microsoft's runtime
and redistribution terms; OpenUnpack's MIT license does not apply to it. The
ordinary installer embeds its bootstrapper and may download the runtime when
missing. The optional offline installer embeds Microsoft's offline installer.
Preserve Microsoft installer terms and allow their normal license/installation
behavior; do not describe either component as project MIT source.

- https://developer.microsoft.com/en-us/microsoft-edge/webview2/
- https://v2.tauri.app/distribute/windows-installer/

Tauri uses NSIS and bundler helper tools to generate Windows setup executables.
Those tools retain their upstream licenses. Generated installer licensing is
not changed by the application's license. See the notices supplied with the
tool downloads and https://nsis.sourceforge.io/License.

## SharpCompress regression fixtures (source/test distributions only)

The synthetic archive samples in `tests/fixtures/upstream-archives/` come from
SharpCompress, commit `2cba09eba4ae400756933807ee5b0aae56bd4fa7`, under MIT.
Keep the adjacent upstream `LICENSE.txt` and `provenance.json` containing exact
source paths, Git blob IDs and SHA-256 values. These archives are test data;
embedded executable bytes are never executed. The samples are included in the
application source release so tests can reproduce format acceptance, and are
excluded from runtime portable/installer packages.

https://github.com/adamhathcock/sharpcompress
