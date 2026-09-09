# Changelog

## [0.2.21](https://github.com/wrightkit/wright/compare/v0.2.20...v0.2.21) (2026-09-09)


### Bug Fixes

* **cli:** render typed results without JSON reflection ([#303](https://github.com/wrightkit/wright/issues/303)) ([3689c4a](https://github.com/wrightkit/wright/commit/3689c4aa1df369b351a8ae8128bd741707ad14d8))
* **distribution:** align Windows installer with R2 ([#285](https://github.com/wrightkit/wright/issues/285)) ([f55f3c1](https://github.com/wrightkit/wright/commit/f55f3c12ff4ec81251f137c509332d877457a891))
* **language:** remove OPY types from shared analysis ([#302](https://github.com/wrightkit/wright/issues/302)) ([efa3831](https://github.com/wrightkit/wright/commit/efa3831b42a0b6770120db9e0f00da465d38ef81))

## [0.2.20](https://github.com/wrightkit/wright/compare/v0.2.19...v0.2.20) (2026-09-09)


### Features

* **distribution:** publish installer archives through R2 ([b75d861](https://github.com/wrightkit/wright/commit/b75d8618311439a229ad24196d690de7db3f93a7)), closes [#261](https://github.com/wrightkit/wright/issues/261)


### Bug Fixes

* **provider:** resolve OPY releases through R2 ([9173ca8](https://github.com/wrightkit/wright/commit/9173ca8833f724899c143940d0de345f05ff2ed9))
* **provider:** use HTTP/2 release client ([cf03383](https://github.com/wrightkit/wright/commit/cf033838eb02a9a85a5a6ce8835a85d28b72655f)), closes [#288](https://github.com/wrightkit/wright/issues/288)
* **release:** keep Homebrew publication independent ([ee20d1c](https://github.com/wrightkit/wright/commit/ee20d1c008fa24d7f266a3dd964512537ce1e69c))
* **release:** namespace R2 publication ([7d5defa](https://github.com/wrightkit/wright/commit/7d5defaedaaa2905845458659c86772a156f597b)), closes [#283](https://github.com/wrightkit/wright/issues/283)
* **update:** defer release client construction ([0e271c5](https://github.com/wrightkit/wright/commit/0e271c51b184d422e42798cd606227e7a8cf7d86)), closes [#288](https://github.com/wrightkit/wright/issues/288)
* **update:** migrate self-update HTTP client ([e49e35d](https://github.com/wrightkit/wright/commit/e49e35d1e803819b2bb5549fe8232238415f72f8)), closes [#288](https://github.com/wrightkit/wright/issues/288)

## [0.2.19](https://github.com/wrightkit/wright/compare/v0.2.18...v0.2.19) (2026-09-08)


### Features

* **analyzer:** flag ongoing condition hot paths ([785c71e](https://github.com/wrightkit/wright/commit/785c71ee5b6f975b2d43e367437ae5229201da86)), closes [#263](https://github.com/wrightkit/wright/issues/263)
* **compat:** gate OPY integration by Workshop semantics ([5b7c6d2](https://github.com/wrightkit/wright/commit/5b7c6d2ebbc3e1a4c82ef49431903d5188adf7f4)), closes [#270](https://github.com/wrightkit/wright/issues/270)


### Bug Fixes

* **analyzer:** preserve condition short circuiting ([0ba1425](https://github.com/wrightkit/wright/commit/0ba142520d22eac0ef2349b5a3470521e46784c4)), closes [#263](https://github.com/wrightkit/wright/issues/263)
* **cli:** route profile selection through driver facade ([d8588af](https://github.com/wrightkit/wright/commit/d8588afe03a1d4f0ee25ae2e1ef6239ea9a10a6c)), closes [#267](https://github.com/wrightkit/wright/issues/267)
* **compat:** consume Workshop-owned semantic equivalence ([c68da9f](https://github.com/wrightkit/wright/commit/c68da9f497e1d514e739909557f371658a45294f))
* **compat:** remove synthetic Workshop equivalence ([6b49ff3](https://github.com/wrightkit/wright/commit/6b49ff323b321ae6af0c5f02550284afc0acbcbd)), closes [#270](https://github.com/wrightkit/wright/issues/270)

## [0.2.18](https://github.com/wrightkit/wright/compare/v0.2.17...v0.2.18) (2026-09-05)


### Features

* **distribution:** add Windows PowerShell installer ([#257](https://github.com/wrightkit/wright/issues/257)) ([2e67354](https://github.com/wrightkit/wright/commit/2e67354acf19b7390c5cf34daaad23e09e5e45ac))
* route OPY workflows through first-party provider ([#252](https://github.com/wrightkit/wright/issues/252)) ([7b88fc0](https://github.com/wrightkit/wright/commit/7b88fc05865b11f3a7d5db4f6e0fc1bf52d4d506))


### Bug Fixes

* **deps:** consume workshop-rs 0.1.18 ([#260](https://github.com/wrightkit/wright/issues/260)) ([64b8877](https://github.com/wrightkit/wright/commit/64b88771650c798979e6b8bface5ce4810542ab4))
* **provider:** match opy release tarballs ([#250](https://github.com/wrightkit/wright/issues/250)) ([d0f61cb](https://github.com/wrightkit/wright/commit/d0f61cba2abac8fd07668268fe06d353e31af6fa))

## [0.2.17](https://github.com/wrightkit/wright/compare/v0.2.16...v0.2.17) (2026-09-02)


### Features

* **driver:** add explicit source provider boundary ([#247](https://github.com/wrightkit/wright/issues/247)) ([958e137](https://github.com/wrightkit/wright/commit/958e137067196de7969f9f31b5748f16988c727a))
* **provider:** resolve first-party OPY providers ([#248](https://github.com/wrightkit/wright/issues/248)) ([b171048](https://github.com/wrightkit/wright/commit/b1710489b36c40a28ad8b896aa909f06b6cca987))

## [0.2.16](https://github.com/wrightkit/wright/compare/v0.2.15...v0.2.16) (2026-08-29)


### Bug Fixes

* **compat:** align OPY 0.1.4 consumer output ([1dc15d5](https://github.com/wrightkit/wright/commit/1dc15d57c32c0c07f39e94ebc3530b506cc611c7)), closes [#228](https://github.com/wrightkit/wright/issues/228)
* **deps:** upgrade OPY owners to 0.1.3 ([d7d0a20](https://github.com/wrightkit/wright/commit/d7d0a20ee122c029a50f1819660d1452e89f4dc5)), closes [#228](https://github.com/wrightkit/wright/issues/228)
* finish Wright source-language cutover follow-ups ([1d17c92](https://github.com/wrightkit/wright/commit/1d17c92714a7ffe0a4ec95db9beb612a4772a0f8))
* pin published DEL owner revision ([9e9165b](https://github.com/wrightkit/wright/commit/9e9165b52e20872bf6b84296ae7c7353d237e2b9))
* pin Windows-safe del-rs revision ([8671810](https://github.com/wrightkit/wright/commit/86718108b83c0182b13d50b23c4b91e69207db00))
* refresh owner dependency lock metadata ([00e4e36](https://github.com/wrightkit/wright/commit/00e4e36f76f435afca7134762085af1e64ee9763))
* run existing adapter integration targets ([7ea3a7a](https://github.com/wrightkit/wright/commit/7ea3a7a700c78a5117b6b1b53ae2f5e37d94d888))
* **workshop:** converge on released workshop-rs surface ([1358fd7](https://github.com/wrightkit/wright/commit/1358fd7f0e436f327c2a8f2a21e64c6f09e5a54f)), closes [#191](https://github.com/wrightkit/wright/issues/191)

## [0.2.15](https://github.com/wrightkit/wright/compare/v0.2.14...v0.2.15) (2026-08-24)


### Bug Fixes

* **workshop:** converge on released workshop-rs 0.1.9 ([#225](https://github.com/wrightkit/wright/issues/225)) ([967c5ad](https://github.com/wrightkit/wright/commit/967c5ad5b801682abd6d9749432adb8a8125b6a6))

## [0.2.14](https://github.com/wrightkit/wright/compare/v0.2.13...v0.2.14) (2026-08-23)


### Features

* **cli:** add phase-aware terminal progress ([#221](https://github.com/wrightkit/wright/issues/221)) ([764300c](https://github.com/wrightkit/wright/commit/764300cff2fa87e1e2df1864c06fe6ee0369816b))

## [0.2.13](https://github.com/wrightkit/wright/compare/v0.2.12...v0.2.13) (2026-08-22)


### Features

* **cli:** make analyze a concise semantic report ([#218](https://github.com/wrightkit/wright/issues/218)) ([562d9cb](https://github.com/wrightkit/wright/commit/562d9cb0a87327094de21b7b7cee4217f1018052))

## [0.2.12](https://github.com/wrightkit/wright/compare/v0.2.11...v0.2.12) (2026-08-22)


### Features

* **cli:** refine interactive terminal presentation ([#216](https://github.com/wrightkit/wright/issues/216)) ([d44d023](https://github.com/wrightkit/wright/commit/d44d023c81b82ca9cfcafeb4febc13f7803874be))

## [0.2.11](https://github.com/wrightkit/wright/compare/v0.2.10...v0.2.11) (2026-08-22)


### Features

* **cli:** separate workflows and improve terminal UX ([ac052ea](https://github.com/wrightkit/wright/commit/ac052ea6217c71a89502984bdf8c94feb668e6fb)), closes [#209](https://github.com/wrightkit/wright/issues/209) [#210](https://github.com/wrightkit/wright/issues/210)


### Bug Fixes

* **ci:** source scenario findings from lint ([d7c7458](https://github.com/wrightkit/wright/commit/d7c745877e5cbb481472032540b03b6099c4bf3e))

## [0.2.10](https://github.com/wrightkit/wright/compare/v0.2.9...v0.2.10) (2026-08-22)


### Features

* integrate raw Workshop LanguageProvider ([fcdc099](https://github.com/wrightkit/wright/commit/fcdc099aa35638c84ca44b83dce319313edc35b6))
* **provider:** define minimal in-process check contract ([#197](https://github.com/wrightkit/wright/issues/197)) ([48c53dc](https://github.com/wrightkit/wright/commit/48c53dc6086c39b29e1cdd07f9c37e7429332157)), closes [#192](https://github.com/wrightkit/wright/issues/192)
* version check JSON diagnostics ([63fa57e](https://github.com/wrightkit/wright/commit/63fa57e7a7ad32cc07c7b258092a8cff362f5ac8))
* **wright:** integrate raw Workshop provider ([85381fe](https://github.com/wrightkit/wright/commit/85381fefe8ffd17615a516e3dd0b266ede06bb72)), closes [#193](https://github.com/wrightkit/wright/issues/193)
* **wright:** version check JSON diagnostics ([180c3ad](https://github.com/wrightkit/wright/commit/180c3ad01a3cbc1c8964c5c1aa0e0a9cb28ec513)), closes [#195](https://github.com/wrightkit/wright/issues/195)


### Bug Fixes

* align tests with workshop-rs 0.1.5 ([eda27e6](https://github.com/wrightkit/wright/commit/eda27e6dd60eb8dbbc3c408fa2324d19304dc4d4))
* **dist:** make verify_tarball rejection assertion platform-agnostic ([7015f14](https://github.com/wrightkit/wright/commit/7015f148bdbd755a5506dca06940c2aea280d5ec))
* update Wright for workshop-rs 0.1.5 ([8b80b13](https://github.com/wrightkit/wright/commit/8b80b13ff21cdee7d355ce401e4a3d539ad92483))
* **wright:** initialize status in CLI test diagnostics ([5771b7b](https://github.com/wrightkit/wright/commit/5771b7ba75b7babcf9c5a7b7047607fa4292274d))
* **wright:** keep provider stack lock reproducible ([1cb345d](https://github.com/wrightkit/wright/commit/1cb345d78fd9215a5877b7370c15fd5ab4f59523))
* **wright:** lock schema test dependencies explicitly ([2f23527](https://github.com/wrightkit/wright/commit/2f235271bec01327781cd23bb1e5d20b8d95825a))
* **wright:** resolve P0 artifacts by owner hash ([cb221e7](https://github.com/wrightkit/wright/commit/cb221e78343cafae28c508e3b3f0b33902275a76))
* **wright:** satisfy provider clippy lint ([e2e76d0](https://github.com/wrightkit/wright/commit/e2e76d00927a87dcab9e0f267df6e47bb1098f45))

## [0.2.9](https://github.com/wrightkit/wright/compare/v0.2.8...v0.2.9) (2026-08-20)


### Features

* **cli:** install and refresh completions through CLI lifecycle ([d03e665](https://github.com/wrightkit/wright/commit/d03e6658f0af092dc784c417dd1313517b84918d)), closes [#186](https://github.com/wrightkit/wright/issues/186)


### Bug Fixes

* integrate raw Workshop P0 convergence ([#190](https://github.com/wrightkit/wright/issues/190)) ([57f415f](https://github.com/wrightkit/wright/commit/57f415f3a87d41209a0527ca3b1a727791530408))

## [0.2.8](https://github.com/wrightkit/wright/compare/v0.2.7...v0.2.8) (2026-08-18)


### Bug Fixes

* **release:** publish canonical release before secondary channels ([#184](https://github.com/wrightkit/wright/issues/184)) ([bea73d2](https://github.com/wrightkit/wright/commit/bea73d2e2b6d28f4f097e0a5b90003ccab6c5a0f))

## [0.2.7](https://github.com/wrightkit/wright/compare/v0.2.6...v0.2.7) (2026-08-18)


### Bug Fixes

* **release:** use explicit GitHub Release repository context ([#176](https://github.com/wrightkit/wright/issues/176)) ([752bb58](https://github.com/wrightkit/wright/commit/752bb589e685f487c8c34779d19acebcab8b836d))

## [0.2.6](https://github.com/wrightkit/wright/compare/v0.2.5...v0.2.6) (2026-08-18)


### Bug Fixes

* **release:** ensure draft GitHub release exists ([#174](https://github.com/wrightkit/wright/issues/174)) ([99dbba1](https://github.com/wrightkit/wright/commit/99dbba1a61399f9cc39f27cd6f54a49749ecdb4c))

## [0.2.5](https://github.com/wrightkit/wright/compare/v0.2.4...v0.2.5) (2026-08-18)


### Features

* **cli:** modernize presentation and CI reporting ([#165](https://github.com/wrightkit/wright/issues/165)) ([281b99a](https://github.com/wrightkit/wright/commit/281b99a963f6cd4796487964e5689751c850105c))
* **release:** migrate Wright lifecycle to release-plz ([#167](https://github.com/wrightkit/wright/issues/167)) ([ba0c3c9](https://github.com/wrightkit/wright/commit/ba0c3c936ec0fa710fde6e26238334071754f9ed)), closes [#166](https://github.com/wrightkit/wright/issues/166)


### Bug Fixes

* **consumer:** preserve event compatibility with workshop-rs ([76c93e7](https://github.com/wrightkit/wright/commit/76c93e7532f19a0632986417b0786cc519916d50))
* **deps:** consume released workshop-rs 0.1.1 ([39b1fdc](https://github.com/wrightkit/wright/commit/39b1fdc5002b7e121dedd4659efcef5a7319cabc)), closes [#163](https://github.com/wrightkit/wright/issues/163)
* **deps:** pin workshop-rs event compatibility revision ([705fa64](https://github.com/wrightkit/wright/commit/705fa64e9d85a0cd0ae8a31bd120fe126c164c00))
* **release:** keep automatic releases on patch versions ([#173](https://github.com/wrightkit/wright/issues/173)) ([57f9452](https://github.com/wrightkit/wright/commit/57f94527705e931746b12bcf823e859f674b55d9))
* **release:** keep Cargo.lock in version bumps ([#161](https://github.com/wrightkit/wright/issues/161)) ([fe38776](https://github.com/wrightkit/wright/commit/fe38776960bca10d3d972217cda07c8763ea1d9e))
* **release:** replace release-plz with release-please ([#170](https://github.com/wrightkit/wright/issues/170)) ([2f6c572](https://github.com/wrightkit/wright/commit/2f6c572e99e5954c184d778c648f0ba7f94c01f1))
* **release:** use compatible release-plz version ([#168](https://github.com/wrightkit/wright/issues/168)) ([09c0f2f](https://github.com/wrightkit/wright/commit/09c0f2ffac38927785f006a0cedbd92026a38295)), closes [#166](https://github.com/wrightkit/wright/issues/166)
* **release:** use organization token for release PR ([#171](https://github.com/wrightkit/wright/issues/171)) ([be7a61b](https://github.com/wrightkit/wright/commit/be7a61b5136bd4d8e4bc532ffbfa07d51565053b)), closes [#169](https://github.com/wrightkit/wright/issues/169)


### Performance Improvements

* **ci:** reuse locked benchmark builds ([e89e725](https://github.com/wrightkit/wright/commit/e89e7254fdcd92ed0dfd2c14fb634a40705014ad)), closes [#160](https://github.com/wrightkit/wright/issues/160)

## Changelog

All notable changes to Wright will be documented in this file.
