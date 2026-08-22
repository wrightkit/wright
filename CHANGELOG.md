# Changelog

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
