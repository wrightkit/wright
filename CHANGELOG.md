# Changelog

## [0.11.0](https://github.com/wrightkit/wright/compare/v0.10.0...v0.11.0) (2026-10-06)


### Features

* **agent:** add the brief result form to analyze, inspect, and lint ([cc535d0](https://github.com/wrightkit/wright/commit/cc535d0c1080741be3bf081647b66c4580e9b87b)), closes [#532](https://github.com/wrightkit/wright/issues/532)
* **agent:** bound and select semantic query results ([46cf32f](https://github.com/wrightkit/wright/commit/46cf32fa0b03894471a27b214ab7b87e76f37c4f)), closes [#531](https://github.com/wrightkit/wright/issues/531)
* **agent:** emit client tool definitions ([beb0bc5](https://github.com/wrightkit/wright/commit/beb0bc593431a1f274085c0db4bf65533ed7b05a)), closes [#535](https://github.com/wrightkit/wright/issues/535)
* **benchmarks:** design and prove seeded defect injection for scenario families ([bfaaa2e](https://github.com/wrightkit/wright/commit/bfaaa2ec1055665a25e70e8fe3d0f8f0d543f0e8)), closes [#534](https://github.com/wrightkit/wright/issues/534)
* **benchmarks:** report scenario discrimination and name smoke scenarios ([6fbf16a](https://github.com/wrightkit/wright/commit/6fbf16ae647e3246b0f8b9ac4371c2890b32d747)), closes [#533](https://github.com/wrightkit/wright/issues/533)
* **benchmarks:** track agent-facing metrics for drift ([7175b56](https://github.com/wrightkit/wright/commit/7175b56d05de6f6668e71b273bcc696240756675)), closes [#530](https://github.com/wrightkit/wright/issues/530)
* **lookup:** owner name and signature lookup ([#529](https://github.com/wrightkit/wright/issues/529)) ([f677b82](https://github.com/wrightkit/wright/commit/f677b82859ae789394115891902908cf591ce751))


### Bug Fixes

* **agent:** keep symbols?kind on its pre-selection shape; normalize latency bands by machine ([241fe9c](https://github.com/wrightkit/wright/commit/241fe9c743980bac569fcbc31b27f7a5e4a3fd51)), closes [#531](https://github.com/wrightkit/wright/issues/531)

## [0.10.0](https://github.com/wrightkit/wright/compare/v0.9.0...v0.10.0) (2026-10-05)


### Features

* **agent:** add first-party MCP bootstrap for coding-agent projects ([0a0a832](https://github.com/wrightkit/wright/commit/0a0a832995ab3950b998b06a40fd19020b51deab)), closes [#509](https://github.com/wrightkit/wright/issues/509)


### Bug Fixes

* **agent:** address review of MCP bootstrap ([f0d3cfe](https://github.com/wrightkit/wright/commit/f0d3cfe32bfead9f49aba977aa6b92dfb606b114))

## [0.9.0](https://github.com/wrightkit/wright/compare/v0.8.0...v0.9.0) (2026-10-05)


### Features

* **bench:** add an mcp tool level beside bin for the agent benchmark ([#492](https://github.com/wrightkit/wright/issues/492)) ([d3e4f1d](https://github.com/wrightkit/wright/commit/d3e4f1d7cd04a3bfa51c9a6775ddf170bec7cd3b))
* **bench:** correction rounds and multi-reference paired lift for the agent benchmark ([#494](https://github.com/wrightkit/wright/issues/494)) ([92179a2](https://github.com/wrightkit/wright/commit/92179a2dc85899ea085a28eb344de49b167e8c0e))
* **bench:** isolate and version the held-out agent suite ([#502](https://github.com/wrightkit/wright/issues/502)) ([2626e91](https://github.com/wrightkit/wright/commit/2626e91f3e07e77035974abe3415c499b7752c62)), closes [#499](https://github.com/wrightkit/wright/issues/499)
* **bench:** publish hosted agent results as immutable R2 data ([#506](https://github.com/wrightkit/wright/issues/506)) ([4bc74cc](https://github.com/wrightkit/wright/commit/4bc74cceb38125647280e0b82f3c791a6cd32d5b)), closes [#500](https://github.com/wrightkit/wright/issues/500)
* **bench:** standardized Wright Agent Score for model comparison ([#491](https://github.com/wrightkit/wright/issues/491)) ([1ad68ec](https://github.com/wrightkit/wright/commit/1ad68ec8a80aa5f5d62d9c0bf2cfdccd3b077e92))
* **cli:** add wright agent install for the canonical agent guide ([e2950d0](https://github.com/wrightkit/wright/commit/e2950d0c0da0bd1cc8e4778f6cc6cfeb74cb5096)), closes [#415](https://github.com/wrightkit/wright/issues/415)
* **driver:** add opt-in hotpath profiling instrumentation ([#507](https://github.com/wrightkit/wright/issues/507)) ([a7e3c0a](https://github.com/wrightkit/wright/commit/a7e3c0aea46c5f8dbdd3c5578053a14f04d1b4ac))
* **driver:** decouple ToolService from successful project loading ([#520](https://github.com/wrightkit/wright/issues/520)) ([56bc689](https://github.com/wrightkit/wright/commit/56bc6897d633f6b4743da1295ac58889a7e6edda))
* **driver:** enforce fixed session configuration after construction ([#519](https://github.com/wrightkit/wright/issues/519)) ([a0f13ec](https://github.com/wrightkit/wright/commit/a0f13ecfaa3ddf68572c6d99d8d21718210e5c40))
* **lsp,language:** wire provider-driven rename through product surfaces ([#498](https://github.com/wrightkit/wright/issues/498)) ([e86ba70](https://github.com/wrightkit/wright/commit/e86ba7038d52eb6fd77313b82f372bd271e0bb30))


### Bug Fixes

* **bench:** sanitize hosted publication validation failures ([#510](https://github.com/wrightkit/wright/issues/510)) ([b5bfa7b](https://github.com/wrightkit/wright/commit/b5bfa7b456a71b20d386ad2b1219d8d68a8dc748))
* **cli:** harden agent install ownership and refresh semantics ([e216dd4](https://github.com/wrightkit/wright/commit/e216dd430b70f25a4533ac53d896681a9b93b98a)), closes [#415](https://github.com/wrightkit/wright/issues/415)
* **driver:** apply canonical Workshop validation to raw Workshop check and compile ([#518](https://github.com/wrightkit/wright/issues/518)) ([40f0f69](https://github.com/wrightkit/wright/commit/40f0f69a10e9e1bb9165e9bd178a64f8ba1bda3a))
* **release:** publish the GitHub Release only after verified R2 staging ([#497](https://github.com/wrightkit/wright/issues/497)) ([60d8bfa](https://github.com/wrightkit/wright/commit/60d8bfaee13a1d19a88b7ce93ce1e5448f93c0c4))


### Performance Improvements

* **analyzer:** cache semantic surfaces per loaded snapshot ([#515](https://github.com/wrightkit/wright/issues/515)) ([62ac4ea](https://github.com/wrightkit/wright/commit/62ac4ea91b95ba3b4ea669e31847f5fce6139194))
* **driver:** reuse loaded semantic state across semantic workflows ([#517](https://github.com/wrightkit/wright/issues/517)) ([da5b3ea](https://github.com/wrightkit/wright/commit/da5b3eaf7f8d7a5f31a1dfc3e41cbaa620edc63b))
* **driver:** share the built-in Workshop catalog across consumers ([#508](https://github.com/wrightkit/wright/issues/508)) ([62adea3](https://github.com/wrightkit/wright/commit/62adea34305a51566edd0dc15551deb523e7e81c))
* **transform:** reuse driver load validation ([#516](https://github.com/wrightkit/wright/issues/516)) ([5e8033d](https://github.com/wrightkit/wright/commit/5e8033df6161bc60bbbfc231707a7d1dab38b293))


### Code Refactoring

* **driver:** deprecate the legacy textual rename helper ([#522](https://github.com/wrightkit/wright/issues/522)) ([b032e09](https://github.com/wrightkit/wright/commit/b032e0966be85fa665d887c9daf1f455c8a30e61))

## [0.8.0](https://github.com/wrightkit/wright/compare/v0.7.0...v0.8.0) (2026-10-02)


### Features

* **cli:** add MCP stdio transport over ToolService ([#481](https://github.com/wrightkit/wright/issues/481)) ([9d46f7d](https://github.com/wrightkit/wright/commit/9d46f7de1da605eb04cf0829a05e8c8667c0260a))
* **compile:** warn when emitted Workshop exceeds the client element limit ([#489](https://github.com/wrightkit/wright/issues/489)) ([43ad5ca](https://github.com/wrightkit/wright/commit/43ad5cab3ed9f975fdf9bbfb88746c5a8565c33f))

## [0.7.0](https://github.com/wrightkit/wright/compare/v0.6.2...v0.7.0) (2026-10-01)


### Features

* **bench:** v3 conditions, overpy tool, usable contract, and the Wright Agent Score ([f31a188](https://github.com/wrightkit/wright/commit/f31a1881f8dd0b56913d6ec67ac496ba789dc0f5))
* **bench:** v3 conditions, Wright Agent Score, and agent adapters ([22fbdda](https://github.com/wrightkit/wright/commit/22fbdda434831dc121dd418b8367ee0b07426b7e))

## [0.6.2](https://github.com/wrightkit/wright/compare/v0.6.1...v0.6.2) (2026-10-01)


### Dependencies

* bump workshop-rs to 1.3.2 ([#485](https://github.com/wrightkit/wright/issues/485)) ([8327f74](https://github.com/wrightkit/wright/commit/8327f743491bf28e7cb3e6d1919c493a469936ba))

## [0.6.1](https://github.com/wrightkit/wright/compare/v0.6.0...v0.6.1) (2026-10-01)


### Dependencies

* bump workshop-rs to 1.3.1 ([#483](https://github.com/wrightkit/wright/issues/483)) ([41e7189](https://github.com/wrightkit/wright/commit/41e7189529d10674aaca58a9d5d3ba1c90274493))

## [0.6.0](https://github.com/wrightkit/wright/compare/v0.5.0...v0.6.0) (2026-10-01)


### Features

* **agent:** default edit sources to on-disk text ([#476](https://github.com/wrightkit/wright/issues/476)) ([c895fd6](https://github.com/wrightkit/wright/commit/c895fd61e7c5e9302e30fce059a31d0209cca4b4))
* **driver:** reload ToolService sessions on disk changes and refuse stale ids ([#478](https://github.com/wrightkit/wright/issues/478)) ([0faa614](https://github.com/wrightkit/wright/commit/0faa61409b3cb5790d58df7257d9892ef73e8bc1)), closes [#471](https://github.com/wrightkit/wright/issues/471)


### Bug Fixes

* **bench:** recalibrate ana-paintball upstream-rejects-name negative ([#479](https://github.com/wrightkit/wright/issues/479)) ([d4f39e2](https://github.com/wrightkit/wright/commit/d4f39e2973fd295abbd8088b02a51afa9fb0a3ba)), closes [#477](https://github.com/wrightkit/wright/issues/477)

## [0.5.0](https://github.com/wrightkit/wright/compare/v0.4.0...v0.5.0) (2026-09-30)


### Features

* **bench:** add condition matrix, tracing, oracle grading, and report to the agent benchmark ([#460](https://github.com/wrightkit/wright/issues/460)) ([905ee94](https://github.com/wrightkit/wright/commit/905ee94410c9b120152900c8dec1b9379f1c9e7e))
* **bench:** add OPY scenarios with oracle-graded checks and negatives ([#461](https://github.com/wrightkit/wright/issues/461)) ([a7904f5](https://github.com/wrightkit/wright/commit/a7904f51a628b15da21dfa01f2e4b15ab727bb56))
* **bench:** add raw Workshop scenarios and a Tier 1 matrix example ([#462](https://github.com/wrightkit/wright/issues/462)) ([833d22e](https://github.com/wrightkit/wright/commit/833d22ed6110a3fa17a26b5a214ef406c59e0ead))
* **driver:** validate and rename raw Workshop edits through workshop-rs ([#454](https://github.com/wrightkit/wright/issues/454)) ([9dcb427](https://github.com/wrightkit/wright/commit/9dcb42794c0bc5f1d502340869af5de05e04a0a7))


### Bug Fixes

* **cli:** resolve wright self-update through the R2 release distribution ([#457](https://github.com/wrightkit/wright/issues/457)) ([25ed555](https://github.com/wrightkit/wright/commit/25ed55559f9f7e3f8f9ec7c413dd83cfd24d3145))


### Dependencies

* bump workshop-rs to 1.2.1 ([#458](https://github.com/wrightkit/wright/issues/458)) ([42ff62d](https://github.com/wrightkit/wright/commit/42ff62deb942c2d7b9c2f121f30e0f8995a42424))

## [0.4.0](https://github.com/wrightkit/wright/compare/v0.3.0...v0.4.0) (2026-09-30)


### Features

* **analyzer:** report identifier spans from workshop-rs provenance ([#438](https://github.com/wrightkit/wright/issues/438)) ([2ce4854](https://github.com/wrightkit/wright/commit/2ce485473a9838b2f829b09274176dee6f9e677b))
* **bench:** add product-level agent benchmark harness ([#428](https://github.com/wrightkit/wright/issues/428)) ([577628b](https://github.com/wrightkit/wright/commit/577628bc4b46fc1a4426c19f831aed87d18cc417)), closes [#414](https://github.com/wrightkit/wright/issues/414)
* **cli:** add symbols/refs/cfg/callgraph/cost query commands ([#440](https://github.com/wrightkit/wright/issues/440)) ([b8c7433](https://github.com/wrightkit/wright/commit/b8c743398900535928f1b948dbbe56ea55c86c38))
* **cli:** consolidate maintenance surface under wright update ([#441](https://github.com/wrightkit/wright/issues/441)) ([fae0de9](https://github.com/wrightkit/wright/commit/fae0de9b50c77387c1a7d395d4c3faff1a67840b))
* **cli:** human-first presentation for inspect queries ([#453](https://github.com/wrightkit/wright/issues/453)) ([c33cad4](https://github.com/wrightkit/wright/commit/c33cad488ef1e1f5f1bfee6c27c250c25466d356))
* **cli:** render check results in a human-first hierarchy ([#447](https://github.com/wrightkit/wright/issues/447)) ([e9fef5f](https://github.com/wrightkit/wright/commit/e9fef5fa3b80ed9173874dff4718bc3b9402f3fb))
* **cli:** render lint findings in a human-first hierarchy ([#450](https://github.com/wrightkit/wright/issues/450)) ([011fca2](https://github.com/wrightkit/wright/commit/011fca2d4d5cbdaf7d94c184d8405c228f07bfb6)), closes [#444](https://github.com/wrightkit/wright/issues/444)
* **cli:** report Workshop cost, hotspots, and risks in analyze ([#451](https://github.com/wrightkit/wright/issues/451)) ([86b1112](https://github.com/wrightkit/wright/commit/86b11120e2f773f30f59de70491d8d3a697146b7)), closes [#445](https://github.com/wrightkit/wright/issues/445)
* **driver:** serve compact rule metadata in the lint result ([#436](https://github.com/wrightkit/wright/issues/436)) ([db42356](https://github.com/wrightkit/wright/commit/db42356008763309642149993600576dc6c366b8))
* **driver:** share finding selection across CLI and agent surfaces ([#435](https://github.com/wrightkit/wright/issues/435)) ([efe6e8c](https://github.com/wrightkit/wright/commit/efe6e8c3e73e3e55ae83c19d452248762565ecd6))


### Bug Fixes

* **analyzer:** scope duplicate-condition to same If/Else If chain ([#437](https://github.com/wrightkit/wright/issues/437)) ([c547756](https://github.com/wrightkit/wright/commit/c547756e28a49f8c3d73181059ff0363ad6f191d))
* **lsp:** advertise only backed wright-lsp capabilities ([#427](https://github.com/wrightkit/wright/issues/427)) ([9ad16c2](https://github.com/wrightkit/wright/commit/9ad16c2c34df1baf0f21bf8f0dddadc81ad782af))


### Performance Improvements

* **driver:** reuse loaded program for Workshop completeness diagnostics ([#448](https://github.com/wrightkit/wright/issues/448)) ([188166a](https://github.com/wrightkit/wright/commit/188166aaf3a2b89780e2a55653c77460b5a14850))

## [0.2.32](https://github.com/wrightkit/wright/compare/v0.2.31...v0.2.32) (2026-09-17)


### Dependencies

* update workshop-rs to 0.4.1 ([#365](https://github.com/wrightkit/wright/issues/365)) ([df20e62](https://github.com/wrightkit/wright/commit/df20e621e6b40c4ad3949a0a54bb1b6ee0020074))

## [0.2.31](https://github.com/wrightkit/wright/compare/v0.2.30...v0.2.31) (2026-09-16)


### Bug Fixes

* **analyzer:** replace serde_yaml with yaml_serde ([#361](https://github.com/wrightkit/wright/issues/361)) ([58c868d](https://github.com/wrightkit/wright/commit/58c868dc19e7e4f021117b97701feabd9b47983e))
* **deps:** enforce single workshop-rs contract ([#362](https://github.com/wrightkit/wright/issues/362)) ([ca2ac58](https://github.com/wrightkit/wright/commit/ca2ac583698ac955607572d9d09ac69e5f497f31))


### Dependencies

* update workshop-rs to 0.3.14 ([#363](https://github.com/wrightkit/wright/issues/363)) ([967f63a](https://github.com/wrightkit/wright/commit/967f63ae882e6e5b773a9f4ad96f1fadd46e1ae6))

## [0.2.30](https://github.com/wrightkit/wright/compare/v0.2.29...v0.2.30) (2026-09-16)


### Bug Fixes

* conform wright-serve to JSON-RPC 2.0 ([8216f8d](https://github.com/wrightkit/wright/commit/8216f8dec6f077345698b0dfabf05dd7aa14b3de)), closes [#357](https://github.com/wrightkit/wright/issues/357)
* support JSON-RPC batch requests ([e6b7416](https://github.com/wrightkit/wright/commit/e6b7416a70b4d7360ae1272ed6e4c3419d28716f)), closes [#357](https://github.com/wrightkit/wright/issues/357)

## [0.2.29](https://github.com/wrightkit/wright/compare/v0.2.28...v0.2.29) (2026-09-15)


### Bug Fixes

* **release:** add failed-release roll-forward entry point ([2cfd9c7](https://github.com/wrightkit/wright/commit/2cfd9c7ef5d3fa45fba1c528c718f76e2f73baba)), closes [#352](https://github.com/wrightkit/wright/issues/352)
* **release:** synchronize recovery release metadata ([87b56d1](https://github.com/wrightkit/wright/commit/87b56d18b2200acad66e060df7f407f2ba91a554)), closes [#352](https://github.com/wrightkit/wright/issues/352)

## [0.2.28](https://github.com/wrightkit/wright/compare/v0.2.27...v0.2.28) (2026-09-15)


### Dependencies

* update workshop-rs to 0.3.11 ([#346](https://github.com/wrightkit/wright/issues/346)) ([7f3876e](https://github.com/wrightkit/wright/commit/7f3876e5e9ba85c5b29216d252511771862e76aa))

## [0.2.27](https://github.com/wrightkit/wright/compare/v0.2.26...v0.2.27) (2026-09-15)


### Bug Fixes

* **release:** restore Windows release packaging ([#341](https://github.com/wrightkit/wright/issues/341)) ([2d09589](https://github.com/wrightkit/wright/commit/2d09589757c7b7676c6e686bca8de54ce1a7335e))

## [0.2.26](https://github.com/wrightkit/wright/compare/v0.2.25...v0.2.26) (2026-09-15)


### Bug Fixes

* **ci:** verify the requested release commit ([1e4c0ea](https://github.com/wrightkit/wright/commit/1e4c0ea3ea6b19a8b33dfea214f6c7d85dda0cd0))

## [0.2.25](https://github.com/wrightkit/wright/compare/v0.2.24...v0.2.25) (2026-09-15)


### Bug Fixes

* **release:** fix exact-SHA CI evidence parsing ([#336](https://github.com/wrightkit/wright/issues/336)) ([ad1ed70](https://github.com/wrightkit/wright/commit/ad1ed70e1d3aba1c21a88b6341efb5493a8c4ccf))

## [0.2.24](https://github.com/wrightkit/wright/compare/v0.2.23...v0.2.24) (2026-09-15)


### Bug Fixes

* **release:** make owner dependency updates releasable ([#331](https://github.com/wrightkit/wright/issues/331)) ([04ff93c](https://github.com/wrightkit/wright/commit/04ff93c491e2abd9f08bced74b53675b13d70186))


### Performance Improvements

* **ci:** remove redundant CI and release work ([#333](https://github.com/wrightkit/wright/issues/333)) ([dbafd1d](https://github.com/wrightkit/wright/commit/dbafd1d3ee47fa4b3fc8456fd05c47b567f129e3))

## [0.2.23](https://github.com/wrightkit/wright/compare/v0.2.22...v0.2.23) (2026-09-14)


### Features

* **input:** support current-directory project targets ([#318](https://github.com/wrightkit/wright/issues/318)) ([98bec23](https://github.com/wrightkit/wright/commit/98bec238026c2cd1ab951258cfa59ffa6f93b11d))
* **wright:** migrate to canonical workshop-rs Program API ([#324](https://github.com/wrightkit/wright/issues/324)) ([167fecc](https://github.com/wrightkit/wright/commit/167feccdf7b6df63f1234d70e2b64f8864896a1a))

## [0.2.22](https://github.com/wrightkit/wright/compare/v0.2.21...v0.2.22) (2026-09-11)


### Features

* analyze persistent Workshop object lifecycle ([#308](https://github.com/wrightkit/wright/issues/308)) ([43d6052](https://github.com/wrightkit/wright/commit/43d605268e45ff6216c41b82e76a11a651bb5cbc))
* **analyzer:** add declarative lint rule contract ([#310](https://github.com/wrightkit/wright/issues/310)) ([db25c08](https://github.com/wrightkit/wright/commit/db25c085c8c6fa88e6e4abfe092cece7f942c2c6))
* **driver:** run OPY lint and analyze through provider ([#304](https://github.com/wrightkit/wright/issues/304)) ([95f0b22](https://github.com/wrightkit/wright/commit/95f0b222c0f3aba01010a826f74b814a0a591374)), closes [#246](https://github.com/wrightkit/wright/issues/246)


### Bug Fixes

* **provider:** make OPY bootstrap concurrency-safe ([#307](https://github.com/wrightkit/wright/issues/307)) ([8d8b4da](https://github.com/wrightkit/wright/commit/8d8b4dafc325cb91034e4bae77fdeebdc73a1ae6))

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
