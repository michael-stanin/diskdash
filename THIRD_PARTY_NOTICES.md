# Third-party notices

diskdash itself is released under the [MIT License](LICENSE). It includes or links the open-source components below, each under its own license. This file is generated from `Cargo.lock` and `frontend/package-lock.json`; the license of every component is in its own package metadata, and license texts are available from each project.

## Components that ask for explicit attribution

- **Apache ECharts** (Apache-2.0): charts. Copyright The Apache Software Foundation. https://echarts.apache.org
- **Geist and Geist Mono fonts** (SIL Open Font License 1.1), bundled through `@fontsource-variable/geist` and `@fontsource-variable/geist-mono`. Copyright Vercel, in collaboration with basement.studio. https://github.com/vercel/geist-font
- **lucide icons** (ISC): copyright Lucide Contributors. https://lucide.dev
- **React, React DOM, React Router, TanStack Query/Table/Virtual, Radix UI, Tailwind CSS** (MIT): copyright their respective authors.
- **SQLite** (public domain), bundled through the `rusqlite` crate. **BLAKE3** (CC0-1.0 / Apache-2.0).
- **windows** crate by Microsoft (MIT OR Apache-2.0).

## Rust dependencies (164 crates in the shipped binaries)

License summary:

- MIT OR Apache-2.0: 93
- MIT: 39
- Unlicense OR MIT: 5
- MIT/Apache-2.0: 5
- Apache-2.0 OR MIT: 5
- Unlicense/MIT: 4
- Apache-2.0: 2
- BSD-2-Clause OR Apache-2.0 OR MIT: 2
- 0BSD OR MIT OR Apache-2.0: 1
- CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception: 1
- CC0-1.0 OR MIT-0 OR Apache-2.0: 1
- MIT AND BSD-3-Clause: 1
- MIT OR Zlib OR Apache-2.0: 1
- Apache-2.0 OR BSL-1.0: 1
- BSD-3-Clause: 1
- (MIT OR Apache-2.0) AND Unicode-3.0: 1
- Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT: 1

| Crate | Version | License |
|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| ahash | 0.8.12 | MIT OR Apache-2.0 |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| arbitrary | 1.5.0 | MIT OR Apache-2.0 |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 |
| askama | 0.12.1 | MIT OR Apache-2.0 |
| askama_derive | 0.12.5 | MIT/Apache-2.0 |
| askama_escape | 0.10.3 | MIT OR Apache-2.0 |
| askama_parser | 0.2.1 | MIT OR Apache-2.0 |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| axum | 0.8.9 | MIT |
| axum-core | 0.5.6 | MIT |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| blake3 | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |
| bstr | 1.13.1 | MIT OR Apache-2.0 |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 |
| bytes | 1.12.1 | MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| cobs | 0.3.0 | MIT OR Apache-2.0 |
| const-oid | 0.10.2 | Apache-2.0 OR MIT |
| constant_time_eq | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |
| csv | 1.4.0 | Unlicense/MIT |
| csv-core | 0.1.13 | Unlicense/MIT |
| derive_arbitrary | 1.5.0 | MIT OR Apache-2.0 |
| digest | 0.11.3 | MIT OR Apache-2.0 |
| embedded-io | 0.6.1 | MIT OR Apache-2.0 |
| embedded-io | 0.4.0 | MIT OR Apache-2.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| errno | 0.3.14 | MIT OR Apache-2.0 |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| futures | 0.3.34 | MIT OR Apache-2.0 |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 |
| futures-core | 0.3.34 | MIT OR Apache-2.0 |
| futures-executor | 0.3.34 | MIT OR Apache-2.0 |
| futures-io | 0.3.34 | MIT OR Apache-2.0 |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 |
| futures-task | 0.3.34 | MIT OR Apache-2.0 |
| futures-util | 0.3.34 | MIT OR Apache-2.0 |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| globset | 0.4.20 | Unlicense OR MIT |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| hashlink | 0.9.1 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| http-body | 1.1.0 | MIT |
| http-body-util | 0.1.5 | MIT |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| httpdate | 1.0.3 | MIT OR Apache-2.0 |
| humantime | 2.4.0 | MIT OR Apache-2.0 |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |
| hyper | 1.12.0 | MIT |
| hyper-util | 0.1.21 | MIT |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 |
| libc | 0.2.190 | MIT OR Apache-2.0 |
| libsqlite3-sys | 0.30.1 | MIT |
| log | 0.4.34 | MIT OR Apache-2.0 |
| matchers | 0.2.0 | MIT |
| matchit | 0.8.4 | MIT AND BSD-3-Clause |
| memchr | 2.8.3 | Unlicense OR MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| mime_guess | 2.0.5 | MIT |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.2.4 | MIT |
| nom | 7.1.3 | MIT |
| nu-ansi-term | 0.50.3 | MIT |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| postcard | 1.1.3 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rand | 0.8.8 | MIT OR Apache-2.0 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rusqlite | 0.32.1 | MIT |
| rust-embed | 8.13.0 | MIT |
| rust-embed-impl | 8.13.0 | MIT |
| rust-embed-utils | 8.13.0 | MIT |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| serde_path_to_error | 0.1.20 | MIT OR Apache-2.0 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 |
| serde_yaml_ng | 0.10.0 | MIT |
| sha2 | 0.11.0 | MIT OR Apache-2.0 |
| sharded-slab | 0.1.7 | MIT |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| slab | 0.4.12 | MIT |
| smallvec | 1.16.3 | MIT OR Apache-2.0 |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |
| subtle | 2.6.1 | BSD-3-Clause |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.8 | MIT OR Apache-2.0 |
| sync_wrapper | 1.0.2 | Apache-2.0 |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| thread_local | 1.1.10 | MIT OR Apache-2.0 |
| tokio | 1.53.2 | MIT |
| tokio-macros | 2.7.2 | MIT |
| tokio-stream | 0.1.19 | MIT |
| tokio-util | 0.7.20 | MIT |
| tower | 0.5.3 | MIT |
| tower-layer | 0.3.3 | MIT |
| tower-service | 0.3.3 | MIT |
| tracing | 0.1.44 | MIT |
| tracing-attributes | 0.1.31 | MIT |
| tracing-core | 0.1.36 | MIT |
| tracing-log | 0.2.0 | MIT |
| tracing-subscriber | 0.3.23 | MIT |
| ts-rs | 10.1.0 | MIT |
| ts-rs-macros | 10.1.0 | MIT |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unicase | 2.10.0 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unsafe-libyaml | 0.2.11 | MIT |
| valuable | 0.1.1 | MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| windows | 0.58.0 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows-core | 0.58.0 | MIT OR Apache-2.0 |
| windows-implement | 0.58.0 | MIT OR Apache-2.0 |
| windows-interface | 0.58.0 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-result | 0.2.0 | MIT OR Apache-2.0 |
| windows-strings | 0.1.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| zerocopy | 0.8.62 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive | 0.8.62 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zip | 2.6.1 | MIT |
| zmij | 1.0.23 | MIT |
| zopfli | 0.8.4 | Apache-2.0 |

## Frontend dependencies (62 packages in the production bundle)

License summary:

- MIT: 53
- 0BSD: 3
- OFL-1.1: 2
- Apache-2.0: 2
- ISC: 1
- BSD-3-Clause: 1

| Package | Version | License |
|---|---|---|
| @floating-ui/core | 1.8.0 | MIT |
| @floating-ui/dom | 1.8.0 | MIT |
| @floating-ui/react-dom | 2.1.9 | MIT |
| @floating-ui/utils | 0.2.12 | MIT |
| @fontsource-variable/geist | 5.3.0 | OFL-1.1 |
| @fontsource-variable/geist-mono | 5.3.0 | OFL-1.1 |
| @radix-ui/primitive | 1.2.0 | MIT |
| @radix-ui/react-arrow | 1.1.17 | MIT |
| @radix-ui/react-compose-refs | 1.1.5 | MIT |
| @radix-ui/react-context | 1.3.0 | MIT |
| @radix-ui/react-dialog | 1.3.0 | MIT |
| @radix-ui/react-dismissable-layer | 1.2.0 | MIT |
| @radix-ui/react-focus-guards | 1.2.0 | MIT |
| @radix-ui/react-focus-scope | 1.3.0 | MIT |
| @radix-ui/react-id | 1.1.4 | MIT |
| @radix-ui/react-popover | 1.3.0 | MIT |
| @radix-ui/react-popper | 1.4.0 | MIT |
| @radix-ui/react-portal | 1.2.0 | MIT |
| @radix-ui/react-presence | 1.1.11 | MIT |
| @radix-ui/react-primitive | 2.2.0 | MIT |
| @radix-ui/react-slot | 1.5.0 | MIT |
| @radix-ui/react-tooltip | 1.4.0 | MIT |
| @radix-ui/react-use-callback-ref | 1.1.5 | MIT |
| @radix-ui/react-use-controllable-state | 1.3.0 | MIT |
| @radix-ui/react-use-effect-event | 0.0.5 | MIT |
| @radix-ui/react-use-layout-effect | 1.1.4 | MIT |
| @radix-ui/react-use-rect | 1.1.4 | MIT |
| @radix-ui/react-use-size | 1.1.5 | MIT |
| @radix-ui/react-visually-hidden | 1.2.13 | MIT |
| @radix-ui/rect | 1.1.3 | MIT |
| @tanstack/query-core | 5.104.2 | MIT |
| @tanstack/react-query | 5.104.2 | MIT |
| @tanstack/react-table | 8.21.3 | MIT |
| @tanstack/react-virtual | 3.14.15 | MIT |
| @tanstack/table-core | 8.21.3 | MIT |
| @tanstack/virtual-core | 3.19.0 | MIT |
| @types/react | 19.3.0 | MIT |
| @types/react-dom | 19.3.0 | MIT |
| aria-hidden | 1.2.6 | MIT |
| class-variance-authority | 0.7.1 | Apache-2.0 |
| clsx | 2.1.1 | MIT |
| cookie | 1.1.1 | MIT |
| csstype | 3.2.3 | MIT |
| detect-node-es | 1.1.0 | MIT |
| echarts | 6.1.0 | Apache-2.0 |
| get-nonce | 1.0.1 | MIT |
| lucide-react | 1.55.0 | ISC |
| react | 19.3.0 | MIT |
| react-dom | 19.3.0 | MIT |
| react-remove-scroll | 2.7.2 | MIT |
| react-remove-scroll-bar | 2.3.8 | MIT |
| react-router | 7.18.4 | MIT |
| react-style-singleton | 2.2.3 | MIT |
| scheduler | 0.28.0 | MIT |
| set-cookie-parser | 2.7.2 | MIT |
| tailwind-merge | 3.7.0 | MIT |
| tslib | 2.3.0 | 0BSD |
| tslib | 2.8.1 | 0BSD |
| tslib | 2.3.0 | 0BSD |
| use-callback-ref | 1.3.3 | MIT |
| use-sidecar | 1.1.3 | MIT |
| zrender | 6.1.0 | BSD-3-Clause |
