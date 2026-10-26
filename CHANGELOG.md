# Changelog

## [0.39.1](https://github.com/wrightkit/wright/compare/v0.39.0...v0.39.1) (2026-10-26)


### Dependencies

* bump athena-sim to v0.5.0 and expose the trace timeline ([#994](https://github.com/wrightkit/wright/issues/994)) ([3d295be](https://github.com/wrightkit/wright/commit/3d295bedd9b96529290e2a389859b2f6049ecb65)), closes [#993](https://github.com/wrightkit/wright/issues/993)

## [0.39.0](https://github.com/wrightkit/wright/compare/v0.38.1...v0.39.0) (2026-10-25)


### ⚠ BREAKING CHANGES

* retire wright ui ([#982](https://github.com/wrightkit/wright/issues/982))

### Features

* retire wright ui ([#982](https://github.com/wrightkit/wright/issues/982)) ([b04229b](https://github.com/wrightkit/wright/commit/b04229b1ee4cb71ec9cf59992fbf7aa0806168e8))


### Bug Fixes

* **cli:** resolve usage-hint subject via the clap error context ([#983](https://github.com/wrightkit/wright/issues/983)) ([6c6e7b2](https://github.com/wrightkit/wright/commit/6c6e7b2de3701774f678726e0b49e863de59269e)), closes [#905](https://github.com/wrightkit/wright/issues/905)

## [0.38.1](https://github.com/wrightkit/wright/compare/v0.38.0...v0.38.1) (2026-10-24)


### Bug Fixes

* **test:** record TestServer requests before writing the response ([#979](https://github.com/wrightkit/wright/issues/979)) ([672e05b](https://github.com/wrightkit/wright/commit/672e05b7d9f68807ea41ef136338186d535cfe6a)), closes [#978](https://github.com/wrightkit/wright/issues/978)


### Code Refactoring

* deduplicate repeated logic and drop dead code ([#981](https://github.com/wrightkit/wright/issues/981)) ([c848e86](https://github.com/wrightkit/wright/commit/c848e867d957061c2264ec15775eb30c77cb1757))

## [0.38.0](https://github.com/wrightkit/wright/compare/v0.37.0...v0.38.0) (2026-10-23)


### Features

* **wasm:** package and publish the WebAssembly core as @wrightkit/core ([#973](https://github.com/wrightkit/wright/issues/973)) ([d697fef](https://github.com/wrightkit/wright/commit/d697fefaf8ed4c126924181ad4bc777e8a48e3f1)), closes [#960](https://github.com/wrightkit/wright/issues/960)

## [0.37.0](https://github.com/wrightkit/wright/compare/v0.36.0...v0.37.0) (2026-10-23)


### Features

* **driver:** refuse unbalanced block structure in editStatements ([#974](https://github.com/wrightkit/wright/issues/974)) ([4948f84](https://github.com/wrightkit/wright/commit/4948f84d3382e06bf5223936b418016517781ea0)), closes [#962](https://github.com/wrightkit/wright/issues/962)

## [0.36.0](https://github.com/wrightkit/wright/compare/v0.35.1...v0.36.0) (2026-10-23)


### Features

* **wasm:** build the Wright core for WebAssembly over in-memory input ([#971](https://github.com/wrightkit/wright/issues/971)) ([cd03235](https://github.com/wrightkit/wright/commit/cd032356f655e723ed071d6b045adf45fc4b2f81)), closes [#959](https://github.com/wrightkit/wright/issues/959) [#951](https://github.com/wrightkit/wright/issues/951)

## [0.35.1](https://github.com/wrightkit/wright/compare/v0.35.0...v0.35.1) (2026-10-23)


### Code Refactoring

* **ui:** move component styles out of app.css into scoped style blocks ([#963](https://github.com/wrightkit/wright/issues/963)) ([db4b779](https://github.com/wrightkit/wright/commit/db4b7794f29dc0de1c938dc27c974b2efb1c7b11)), closes [#928](https://github.com/wrightkit/wright/issues/928)

## [0.35.0](https://github.com/wrightkit/wright/compare/v0.34.0...v0.35.0) (2026-10-23)


### Features

* **ui:** argument hover card on statement rows ([#950](https://github.com/wrightkit/wright/issues/950)) ([61ad922](https://github.com/wrightkit/wright/commit/61ad922de9bbc5268f6b76dc4538b02f53b697d7)), closes [#924](https://github.com/wrightkit/wright/issues/924)
* **ui:** insert a statement from the column heading ([#956](https://github.com/wrightkit/wright/issues/956)) ([3d72c23](https://github.com/wrightkit/wright/commit/3d72c2363f5df8351c0dbbb15b33fefa0e54e97e)), closes [#920](https://github.com/wrightkit/wright/issues/920)
* **ui:** pickers for actions, values, enums, variables, and operators ([#957](https://github.com/wrightkit/wright/issues/957)) ([1ba467a](https://github.com/wrightkit/wright/commit/1ba467ac0f31a6973011609ea2e504d3453f93e2)), closes [#919](https://github.com/wrightkit/wright/issues/919)
* **ui:** statement editor form for an existing statement ([#949](https://github.com/wrightkit/wright/issues/949)) ([4576068](https://github.com/wrightkit/wright/commit/4576068355048cd9071d7b50a1d9d9428b02dec9)), closes [#918](https://github.com/wrightkit/wright/issues/918)

## [0.34.0](https://github.com/wrightkit/wright/compare/v0.33.0...v0.34.0) (2026-10-23)


### Features

* **ui:** select same action and select block ([#947](https://github.com/wrightkit/wright/issues/947)) ([44897bb](https://github.com/wrightkit/wright/commit/44897bba8e7d1d73588f0ab0bf304fbf3ae40ec0)), closes [#917](https://github.com/wrightkit/wright/issues/917)

## [0.33.0](https://github.com/wrightkit/wright/compare/v0.32.0...v0.33.0) (2026-10-22)


### Features

* **analyzer:** give references a statement-span fallback ([#892](https://github.com/wrightkit/wright/issues/892)) ([cab8444](https://github.com/wrightkit/wright/commit/cab84444fe3ce17c8c10270176953401b0cf9018))
* **ui:** keep column batch actions in sticky headings ([#940](https://github.com/wrightkit/wright/issues/940)) ([fa95feb](https://github.com/wrightkit/wright/commit/fa95feb9cbbd4acc3ad55802643e5981f2ec7df4)), closes [#915](https://github.com/wrightkit/wright/issues/915)
* **ui:** replace hand-drawn icons with Lucide ([#937](https://github.com/wrightkit/wright/issues/937)) ([6cd01d4](https://github.com/wrightkit/wright/commit/6cd01d48ed545caf51db39a4e282f7654a8d3d44)), closes [#929](https://github.com/wrightkit/wright/issues/929)
* **ui:** severity levels in the toolbar and the Diagnostics dialog ([#946](https://github.com/wrightkit/wright/issues/946)) ([7c0ceb4](https://github.com/wrightkit/wright/commit/7c0ceb49edac41d74853fbff3fb5a033ff350431)), closes [#922](https://github.com/wrightkit/wright/issues/922)
* **ui:** show findings on rule bars and statement rows ([#943](https://github.com/wrightkit/wright/issues/943)) ([06c2019](https://github.com/wrightkit/wright/commit/06c201909a24a9aa5edc92783401f9fc51af5dab)), closes [#921](https://github.com/wrightkit/wright/issues/921)


### Bug Fixes

* **cli:** put a failed result's diagnostics on stdout ([#891](https://github.com/wrightkit/wright/issues/891)) ([54f50f4](https://github.com/wrightkit/wright/commit/54f50f43705eb6bd43faa866c5028554926913d0))
* **ui:** scope column keyboard gestures to the owning rule ([#945](https://github.com/wrightkit/wright/issues/945)) ([57fe268](https://github.com/wrightkit/wright/commit/57fe2680fc5f0ca4e4bf93a234fc47cf0cb98bfc)), closes [#916](https://github.com/wrightkit/wright/issues/916)


### Dependencies

* consume whole-statement spans from workshop-rs ([#938](https://github.com/wrightkit/wright/issues/938)) ([f9c6e79](https://github.com/wrightkit/wright/commit/f9c6e79667337e29a4575c386f592e7ce5279856)), closes [#925](https://github.com/wrightkit/wright/issues/925)


### Code Refactoring

* state comment-only behavior in names, types, and CI ([#944](https://github.com/wrightkit/wright/issues/944)) ([666e689](https://github.com/wrightkit/wright/commit/666e6890888b2309c97a5b9c2801e7b8095654f7)), closes [#874](https://github.com/wrightkit/wright/issues/874)

## [0.32.0](https://github.com/wrightkit/wright/compare/v0.31.0...v0.32.0) (2026-10-22)


### Features

* **cli:** self-update standalone Windows installs ([#930](https://github.com/wrightkit/wright/issues/930)) ([a9c484b](https://github.com/wrightkit/wright/commit/a9c484b4c3f958c9783dd168bf94da2c4e4805e7))

## [0.31.0](https://github.com/wrightkit/wright/compare/v0.30.0...v0.31.0) (2026-10-22)


### Features

* **ui:** argument forms and dropdowns for the statement editor ([0929298](https://github.com/wrightkit/wright/commit/0929298c79ddb66ffedf315739a1089ca2781d1d)), closes [#835](https://github.com/wrightkit/wright/issues/835)
* **ui:** expose the settings block and add the simulation panel ([f739b18](https://github.com/wrightkit/wright/commit/f739b1867884c045c3ed2f3d40a5bd1e867c41d5)), closes [#836](https://github.com/wrightkit/wright/issues/836)


### Bug Fixes

* **ui:** cover the simulate refusal path and Picker keyboard navigation ([e07967b](https://github.com/wrightkit/wright/commit/e07967ba9d0e9e02020f9894285746ff9429b1d5)), closes [#836](https://github.com/wrightkit/wright/issues/836)
* **ui:** let a failed hero listing retry on the next open ([013050d](https://github.com/wrightkit/wright/commit/013050d9605a56cb87726252d0664fe35d7691ec)), closes [#836](https://github.com/wrightkit/wright/issues/836)

## [0.30.0](https://github.com/wrightkit/wright/compare/v0.29.0...v0.30.0) (2026-10-22)


### Features

* **analyzer:** select rules and symbols by ADR-0035 address grammar ([#903](https://github.com/wrightkit/wright/issues/903)) ([f6229b0](https://github.com/wrightkit/wright/commit/f6229b04f1677b07569aed4cd74cfbfdb4d7f47c)), closes [#895](https://github.com/wrightkit/wright/issues/895)
* **cli:** give each selector spelling one meaning per ADR-0035 ([#902](https://github.com/wrightkit/wright/issues/902)) ([24f60f2](https://github.com/wrightkit/wright/commit/24f60f2cdb284fcfb9df9fa37cc5b3ca670106a5)), closes [#889](https://github.com/wrightkit/wright/issues/889)
* **driver:** add editStatements operation with rule-content nodes ([f4d5171](https://github.com/wrightkit/wright/commit/f4d5171593513a779b2a840647e166ab0a4b0734)), closes [#855](https://github.com/wrightkit/wright/issues/855)
* **driver:** add simulate operation backed by athena ([#893](https://github.com/wrightkit/wright/issues/893)) ([baca6fc](https://github.com/wrightkit/wright/commit/baca6fccb9f0b37a8b52e354ef6bcd3905816a10)), closes [#833](https://github.com/wrightkit/wright/issues/833)
* **driver:** assert the diagnostic-code grammar at code constructors ([#906](https://github.com/wrightkit/wright/issues/906)) ([a57bdaf](https://github.com/wrightkit/wright/commit/a57bdafc1aa6f791b301b05ff215c0eec6c91613)), closes [#871](https://github.com/wrightkit/wright/issues/871)
* **service:** name the working command in selector and usage errors ([b898dc3](https://github.com/wrightkit/wright/commit/b898dc3d41884c48ce5da7585896c7ec64bdaf58)), closes [#888](https://github.com/wrightkit/wright/issues/888)
* **ui:** add session locale and locale selector ([#901](https://github.com/wrightkit/wright/issues/901)) ([021736a](https://github.com/wrightkit/wright/commit/021736a8e4c139f971a581490da61e14f79f4089)), closes [#883](https://github.com/wrightkit/wright/issues/883)
* **ui:** add the editing layer's statement edits ([#894](https://github.com/wrightkit/wright/issues/894)) ([ebc16b4](https://github.com/wrightkit/wright/commit/ebc16b4daa28c9ae7862c6b335de89199fa9ef89)), closes [#834](https://github.com/wrightkit/wright/issues/834)
* **ui:** diagnostics and variables dialogs ([#900](https://github.com/wrightkit/wright/issues/900)) ([b511844](https://github.com/wrightkit/wright/commit/b511844c79279b05f27c23938bb1be26e6f5c05a)), closes [#882](https://github.com/wrightkit/wright/issues/882)
* **ui:** give wright ui the in-game page shell ([#896](https://github.com/wrightkit/wright/issues/896)) ([d1fb619](https://github.com/wrightkit/wright/commit/d1fb61929311f9fffc45f327c13f94df29710da8)), closes [#879](https://github.com/wrightkit/wright/issues/879)
* **ui:** open rule body per design spec ([#898](https://github.com/wrightkit/wright/issues/898)) ([1c2dd16](https://github.com/wrightkit/wright/commit/1c2dd165c4d430e41907c8fceeeb1b9612c4fc00)), closes [#881](https://github.com/wrightkit/wright/issues/881)
* **ui:** rules as expanding bars ([#897](https://github.com/wrightkit/wright/issues/897)) ([a83c92a](https://github.com/wrightkit/wright/commit/a83c92a4aee5fa54bf8f244d3430a50dd07b8ca7)), closes [#880](https://github.com/wrightkit/wright/issues/880)


### Bug Fixes

* **bench:** replace the absolute token ceiling with the drift band ([#885](https://github.com/wrightkit/wright/issues/885)) ([a616c01](https://github.com/wrightkit/wright/commit/a616c01ab22fc3651da28b3085839e4d58983fd8))


### Code Refactoring

* remove unsafe code and deny it workspace-wide ([#907](https://github.com/wrightkit/wright/issues/907)) ([77659e8](https://github.com/wrightkit/wright/commit/77659e8a7cfb36a9abd1aa5a77cebd23b34b8bd7)), closes [#887](https://github.com/wrightkit/wright/issues/887)

## [0.29.0](https://github.com/wrightkit/wright/compare/v0.28.0...v0.29.0) (2026-10-20)


### Features

* add ui/ Svelte 5 toolchain with generated types and UI CI job ([#813](https://github.com/wrightkit/wright/issues/813)) ([d3e3787](https://github.com/wrightkit/wright/commit/d3e37879c2751a60da574b6abd25af9cbcc51a1c)), closes [#802](https://github.com/wrightkit/wright/issues/802)
* add wright lint package commands ([#861](https://github.com/wrightkit/wright/issues/861)) ([328eb80](https://github.com/wrightkit/wright/commit/328eb8067d8f3478e9fdf35a2fb904e31a535d10))
* **cli:** add wright ui command and loopback endpoint ([#827](https://github.com/wrightkit/wright/issues/827)) ([6e774f6](https://github.com/wrightkit/wright/commit/6e774f66ca95d0ded4f52fee09a8bcf2f61743a3))
* **cli:** bounded GitHub Actions annotations, step summary, and log group ([#830](https://github.com/wrightkit/wright/issues/830)) ([5db4ae3](https://github.com/wrightkit/wright/commit/5db4ae3b5020fe575612ba8042cd24c13e14e911))
* **cli:** opt-in --fail-on gate that raises the verdict at a severity threshold ([#810](https://github.com/wrightkit/wright/issues/810)) ([a2d6dd4](https://github.com/wrightkit/wright/commit/a2d6dd477fa6b19348b191b03000eb14b65c64ff)), closes [#796](https://github.com/wrightkit/wright/issues/796)
* **cli:** plain line-record renderer for agents and CI ([#828](https://github.com/wrightkit/wright/issues/828)) ([2c62751](https://github.com/wrightkit/wright/commit/2c6275167250063c962eb427df53903bd3094955))
* **cli:** replace the terminal rail with the verdict-tag report ([#831](https://github.com/wrightkit/wright/issues/831)) ([74d3bae](https://github.com/wrightkit/wright/commit/74d3bae9ee0f13dbb786c7565bc70fb0c69dcf21))
* **driver:** add the listEntries operation for structured editors ([#856](https://github.com/wrightkit/wright/issues/856)) ([126a104](https://github.com/wrightkit/wright/commit/126a10443c0adb95aed886ca7b196824c8577cf2)), closes [#854](https://github.com/wrightkit/wright/issues/854)
* **driver:** expose repeated-value facts and pre-truncation per-code counts ([#820](https://github.com/wrightkit/wright/issues/820)) ([2843ffd](https://github.com/wrightkit/wright/commit/2843ffd66db925d66cb7606c85134dc15830f28b)), closes [#791](https://github.com/wrightkit/wright/issues/791)
* **driver:** report check's program summary, shared follow-ups, and ref rule identity ([#821](https://github.com/wrightkit/wright/issues/821)) ([68f9f50](https://github.com/wrightkit/wright/commit/68f9f5089a82a1857d22599b23f4deeba1ee26c6))
* **driver:** scope numeric symbol/rule addresses to a snapshot generation ([#847](https://github.com/wrightkit/wright/issues/847)) ([02cd5e7](https://github.com/wrightkit/wright/commit/02cd5e7a05bd52d9e7c092c29b3d0488bd4fe92a))
* fetch and store GitHub lint rule packages by commit SHA ([#857](https://github.com/wrightkit/wright/issues/857)) ([05c3537](https://github.com/wrightkit/wright/commit/05c3537820bb7da5a030f7ef37ead5b9e1c1286e))
* pin lint rule packages per project by commit SHA ([#858](https://github.com/wrightkit/wright/issues/858)) ([aa2e184](https://github.com/wrightkit/wright/commit/aa2e184a9cbdba557a61574418b16bb12e04b7af))
* **ui:** structured read-only Workshop rule view ([#829](https://github.com/wrightkit/wright/issues/829)) ([8435938](https://github.com/wrightkit/wright/commit/8435938aa6ef6b5796f7a5e1b7d348456d74f572))


### Bug Fixes

* **cost:** report failed computations as unavailable, never zero ([#816](https://github.com/wrightkit/wright/issues/816)) ([8f92809](https://github.com/wrightkit/wright/commit/8f9280960cefc5cbbec20d0b4ecb44f4c4efa07e)), closes [#790](https://github.com/wrightkit/wright/issues/790)
* **driver:** reconcile analyze and inspect cost counts ([#819](https://github.com/wrightkit/wright/issues/819)) ([abf6baf](https://github.com/wrightkit/wright/commit/abf6baf314fb7709cdf792e7b765e4f99fb45cb4))
* **lookup:** reject an out-of-range limit instead of clamping silently ([#811](https://github.com/wrightkit/wright/issues/811)) ([58ae2cb](https://github.com/wrightkit/wright/commit/58ae2cba31216260617fcee3c4b7e7f1042c02d6)), closes [#783](https://github.com/wrightkit/wright/issues/783)
* **ui:** keep connections alive in `wright ui` ([#864](https://github.com/wrightkit/wright/issues/864)) ([b957ce9](https://github.com/wrightkit/wright/commit/b957ce9a8c7852f30d1a96469c7d89415044ab04))
* **ui:** keep the selected rule and wait for the initial load in E2E ([#863](https://github.com/wrightkit/wright/issues/863)) ([53e4e19](https://github.com/wrightkit/wright/commit/53e4e1907f51563780d6008d6782a823c26e80d3))

## [0.28.0](https://github.com/wrightkit/wright/compare/v0.27.0...v0.28.0) (2026-10-19)


### ⚠ BREAKING CHANGES

* space-carrying diagnostic codes are renamed: unknown-enum member -> unknown-enum-member, and likewise for the other phrased owner kinds (unknown-enum-domain, unknown-localized-string, unknown-event-player, unknown-event-team, unknown-setting-boolean, unknown-modify-operator, unknown-settings-enum, unknown-top-level-section, unknown-rule-section, unknown-lookup-scope, unknown-global-variable, unknown-player-variable).
* remove semantic rename from CLI and agent surfaces ([#801](https://github.com/wrightkit/wright/issues/801))

### Features

* **lookup:** carry owner return type and variadic on callable facts ([#807](https://github.com/wrightkit/wright/issues/807)) ([d51340c](https://github.com/wrightkit/wright/commit/d51340cce39718f04d26afe271bcc692df3d69e3)), closes [#781](https://github.com/wrightkit/wright/issues/781) [#798](https://github.com/wrightkit/wright/issues/798)
* read and write .wright/ baselines under CI, degrade silently ([#809](https://github.com/wrightkit/wright/issues/809)) ([0308150](https://github.com/wrightkit/wright/commit/0308150c61f6b84235ba6a281e9a1f8bfea2aab6)), closes [#797](https://github.com/wrightkit/wright/issues/797)
* remove semantic rename from CLI and agent surfaces ([#801](https://github.com/wrightkit/wright/issues/801)) ([befabc2](https://github.com/wrightkit/wright/commit/befabc202d59790e477accaf8d00a8fef96185d6))


### Bug Fixes

* hyphenate phrased Workshop error kinds in diagnostic codes ([#808](https://github.com/wrightkit/wright/issues/808)) ([03b7b9d](https://github.com/wrightkit/wright/commit/03b7b9d94f298b03fec13b2be6a65962fb62204c)), closes [#789](https://github.com/wrightkit/wright/issues/789)

## [0.27.0](https://github.com/wrightkit/wright/compare/v0.26.3...v0.27.0) (2026-10-19)


### Features

* **install:** add the default install directory to the user PATH ([#778](https://github.com/wrightkit/wright/issues/778)) ([831a5db](https://github.com/wrightkit/wright/commit/831a5dbf19f76fe94aeea4cdd58d399e6a6f0304))

## [0.26.3](https://github.com/wrightkit/wright/compare/v0.26.2...v0.26.3) (2026-10-19)


### Performance Improvements

* **deps:** require workshop-rs 1.18.0 ([#776](https://github.com/wrightkit/wright/issues/776)) ([fe04237](https://github.com/wrightkit/wright/commit/fe04237d3e234887835fce38beaef19f867f4113)), closes [#775](https://github.com/wrightkit/wright/issues/775)

## [0.26.2](https://github.com/wrightkit/wright/compare/v0.26.1...v0.26.2) (2026-10-19)


### Bug Fixes

* **install:** detect Windows architecture from the environment ([#773](https://github.com/wrightkit/wright/issues/773)) ([262471a](https://github.com/wrightkit/wright/commit/262471a3b00bf1e8c0795d9311a104cf2458f221))

## [0.26.1](https://github.com/wrightkit/wright/compare/v0.26.0...v0.26.1) (2026-10-19)


### Dependencies

* adopt workshop-rs shared built-in catalog cache ([#771](https://github.com/wrightkit/wright/issues/771)) ([e36bba6](https://github.com/wrightkit/wright/commit/e36bba6ebebe6b088463273d9452440b95ffd550))

## [0.26.0](https://github.com/wrightkit/wright/compare/v0.25.2...v0.26.0) (2026-10-18)


### Features

* **compile:** add `--to <LOCALE>` for cross-locale Workshop emission ([#760](https://github.com/wrightkit/wright/issues/760)) ([e958526](https://github.com/wrightkit/wright/commit/e9585265f846a99eb730a8498d975a9ee5dc55ad))


### Bug Fixes

* **driver:** honor non-empty semantics for WRIGHT_NO_CACHE ([#767](https://github.com/wrightkit/wright/issues/767)) ([b45cc83](https://github.com/wrightkit/wright/commit/b45cc83df47dffd5867e0d266a303a386ce5ca00))
* **driver:** validate --to locale before loading the project ([#768](https://github.com/wrightkit/wright/issues/768)) ([74dbb6b](https://github.com/wrightkit/wright/commit/74dbb6bce0b493e42a9f53b9e02fc1d69371b5ab))
* **lpp:** reject non-UTF-8 and overlong provider lines as protocol violations ([#765](https://github.com/wrightkit/wright/issues/765)) ([bd4084b](https://github.com/wrightkit/wright/commit/bd4084b78d274f057c637c233c472ceb5128bf1c)), closes [#764](https://github.com/wrightkit/wright/issues/764)


### Code Refactoring

* retire in-process provider abstraction and reclaim audit entropy ([#763](https://github.com/wrightkit/wright/issues/763)) ([0504086](https://github.com/wrightkit/wright/commit/0504086f01bf362a6351529946c5df749936811b))

## [0.25.2](https://github.com/wrightkit/wright/compare/v0.25.1...v0.25.2) (2026-10-18)


### Bug Fixes

* **cli:** locate rejected Workshop setting values and name their locale in the lookup hint ([#757](https://github.com/wrightkit/wright/issues/757)) ([a813597](https://github.com/wrightkit/wright/commit/a8135971d3bb76cc2a0bc4207ba5594048519cbf))

## [0.25.1](https://github.com/wrightkit/wright/compare/v0.25.0...v0.25.1) (2026-10-18)


### Code Refactoring

* **analyzer:** remove dead reindexing and duplicate rule facts ([#750](https://github.com/wrightkit/wright/issues/750)) ([85db92f](https://github.com/wrightkit/wright/commit/85db92fb003fea1e32747d4077655178de59b691))
* **cli:** drop dead provider_workflow parameter ([#748](https://github.com/wrightkit/wright/issues/748)) ([3086324](https://github.com/wrightkit/wright/commit/30863245e6662630cdac8a27265853ed48b6356a))
* **driver:** share cost-cache construction and group snapshot state ([#751](https://github.com/wrightkit/wright/issues/751)) ([a9cb17f](https://github.com/wrightkit/wright/commit/a9cb17f51ce9e28907fc753de5bc119e489b9119))
* **driver:** share lint findings preparation ([#755](https://github.com/wrightkit/wright/issues/755)) ([2b2b4e4](https://github.com/wrightkit/wright/commit/2b2b4e4cf3c4eed47a85564a08f5106c30452856))
* reuse source_document_set in rename path ([#749](https://github.com/wrightkit/wright/issues/749)) ([e0468e6](https://github.com/wrightkit/wright/commit/e0468e6a6c37d0f547d8a093e27c4723bc8fb539))

## [0.25.0](https://github.com/wrightkit/wright/compare/v0.24.0...v0.25.0) (2026-10-18)


### Features

* **driver,cli:** rename OPY input through the provider ([#746](https://github.com/wrightkit/wright/issues/746)) ([fba0364](https://github.com/wrightkit/wright/commit/fba0364709018f6f0d41f62134e2862e89d44b4a)), closes [#739](https://github.com/wrightkit/wright/issues/739)
* **lsp:** provider-backed OPY diagnostics and navigation ([#738](https://github.com/wrightkit/wright/issues/738)) ([#745](https://github.com/wrightkit/wright/issues/745)) ([ccddb4f](https://github.com/wrightkit/wright/commit/ccddb4f07dc2c5c6aa0d09c65c3f8741517bbdc1))


### Bug Fixes

* **analyzer:** name the repeated expression in repeated-value findings ([#743](https://github.com/wrightkit/wright/issues/743)) ([e8fc9d3](https://github.com/wrightkit/wright/commit/e8fc9d3346758520d3e30d36a3249a49350f50e1))
* **driver:** roll back committed writes when a later edit source fails ([#742](https://github.com/wrightkit/wright/issues/742)) ([1488884](https://github.com/wrightkit/wright/commit/1488884ccc6d0e24d35843eda8e8aed7c864f8c9)), closes [#734](https://github.com/wrightkit/wright/issues/734)

## [0.24.0](https://github.com/wrightkit/wright/compare/v0.23.0...v0.24.0) (2026-10-17)


### ⚠ BREAKING CHANGES

* restrict --profile to Workshop input and remove aggressive ([#733](https://github.com/wrightkit/wright/issues/733))

### Features

* **cli,driver:** locate inspect cost rules/findings and add per-rule drill-down ([#730](https://github.com/wrightkit/wright/issues/730)) ([a46971c](https://github.com/wrightkit/wright/commit/a46971c70fdf7e183885de0c4ece2aecb2bf8560))
* **driver,cli:** project run cache and cost comparison against the previous run ([#731](https://github.com/wrightkit/wright/issues/731)) ([cf9f947](https://github.com/wrightkit/wright/commit/cf9f9477fb118d6cdb8d75496846453828b54b8c))
* restrict --profile to Workshop input and remove aggressive ([#733](https://github.com/wrightkit/wright/issues/733)) ([fba2cd4](https://github.com/wrightkit/wright/commit/fba2cd404f6a7330ec2c66a7b1bcc72624cbdd27)), closes [#725](https://github.com/wrightkit/wright/issues/725)

## [0.23.0](https://github.com/wrightkit/wright/compare/v0.22.2...v0.23.0) (2026-10-17)


### Features

* **cli:** draw terminal reports on one continuous rail ([#719](https://github.com/wrightkit/wright/issues/719)) ([8b46b01](https://github.com/wrightkit/wright/commit/8b46b01341370f7a40b9af16d385f70e57f84340))
* **lint:** add nested-binder-lambda import-risk indicator ([#721](https://github.com/wrightkit/wright/issues/721)) ([f1f86dc](https://github.com/wrightkit/wright/commit/f1f86dc2466e14e226f057d2c6342f3179bb733f))

## [0.22.2](https://github.com/wrightkit/wright/compare/v0.22.1...v0.22.2) (2026-10-16)


### Bug Fixes

* **distribution:** drop forced HTTP/2 from the Windows installer ([#715](https://github.com/wrightkit/wright/issues/715)) ([714b197](https://github.com/wrightkit/wright/commit/714b197875c8f09171013d2c419f0702cfa6b180))

## [0.22.1](https://github.com/wrightkit/wright/compare/v0.22.0...v0.22.1) (2026-10-16)


### Code Refactoring

* strip narrating comments and encode constraints structurally ([#708](https://github.com/wrightkit/wright/issues/708)) ([2b84a6a](https://github.com/wrightkit/wright/commit/2b84a6aebce665dc9080067a5a14bb64d1fe884f))

## [0.22.0](https://github.com/wrightkit/wright/compare/v0.21.1...v0.22.0) (2026-10-14)


### Features

* **cli:** delimit terminal report items with lightweight connectors ([#705](https://github.com/wrightkit/wright/issues/705)) ([43b0b0f](https://github.com/wrightkit/wright/commit/43b0b0fe557328a6df5ab92548f7a44a529e9ca5)), closes [#703](https://github.com/wrightkit/wright/issues/703)

## [0.21.1](https://github.com/wrightkit/wright/compare/v0.21.0...v0.21.1) (2026-10-14)


### Bug Fixes

* **dist:** serve loopback test fixtures without reverse-DNS lookup ([#700](https://github.com/wrightkit/wright/issues/700)) ([6241833](https://github.com/wrightkit/wright/commit/624183348ec42c6c9e42b9bc5829f3a3836f6cd9))

## [0.21.0](https://github.com/wrightkit/wright/compare/v0.20.2...v0.21.0) (2026-10-14)


### Features

* **driver:** report the canonical element count in costEstimate ([#694](https://github.com/wrightkit/wright/issues/694)) ([ccab9e2](https://github.com/wrightkit/wright/commit/ccab9e2718fb175258e74d92dd5dab56b699e57e))


### Bug Fixes

* **cli:** point the lookup hint at the OPY vocabulary for OPY-source diagnostics ([#693](https://github.com/wrightkit/wright/issues/693)) ([2f61a73](https://github.com/wrightkit/wright/commit/2f61a7356e003fa1f9fc328cad8b6e87126bf961)), closes [#692](https://github.com/wrightkit/wright/issues/692)

## [0.20.2](https://github.com/wrightkit/wright/compare/v0.20.1...v0.20.2) (2026-10-14)


### Dependencies

* **agent:** sync vendored guide to skills@bf94300 ([#689](https://github.com/wrightkit/wright/issues/689)) ([4e6423c](https://github.com/wrightkit/wright/commit/4e6423c83e0d1548b8843c7a0e9af0f5005c865d))


### Code Refactoring

* **driver,cli:** remove duplicated snapshot, severity and span-path code ([#687](https://github.com/wrightkit/wright/issues/687)) ([e798311](https://github.com/wrightkit/wright/commit/e7983113dd1b9e72e23f66bf2e0c1252592e7851))

## [0.20.1](https://github.com/wrightkit/wright/compare/v0.20.0...v0.20.1) (2026-10-13)


### Code Refactoring

* **bench:** move the agent benchmark harness to benchmark-suite ([#682](https://github.com/wrightkit/wright/issues/682)) ([8c5fbdc](https://github.com/wrightkit/wright/commit/8c5fbdc2a0c2d061004a2fbd4f1d025c21674af4)), closes [#670](https://github.com/wrightkit/wright/issues/670)

## [0.20.0](https://github.com/wrightkit/wright/compare/v0.19.0...v0.20.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* pre-1.0 wright-result/v1 and wright-agent/v1 results default to essential content; brief is removed and all requests complete lists.
* **cli:** result.output.text is removed from the wright-result/v1 compile result and compile no longer prints the program to stdout by default (ADR-0025).

### Features

* **bench:** record scenario-cost metrics for the recommended call sequences ([#681](https://github.com/wrightkit/wright/issues/681)) ([b66c155](https://github.com/wrightkit/wright/commit/b66c155c278453991e3050e83337ff4934c9d9d0)), closes [#671](https://github.com/wrightkit/wright/issues/671)
* **cli:** write compile output to a file ([#678](https://github.com/wrightkit/wright/issues/678)) ([c95371d](https://github.com/wrightkit/wright/commit/c95371d83154a27bf2174b29531cffafbc4d128e))
* default to essential output on every surface ([#677](https://github.com/wrightkit/wright/issues/677)) ([d137029](https://github.com/wrightkit/wright/commit/d13702923d5ff8376520cafe0e693f2fb325e6ce))


### Code Refactoring

* **bench:** decouple product tests and metrics corpus from benchmarks/agent ([#676](https://github.com/wrightkit/wright/issues/676)) ([f28beb0](https://github.com/wrightkit/wright/commit/f28beb007308ab9b0a77f35701c03780cd452715)), closes [#669](https://github.com/wrightkit/wright/issues/669)

## [0.19.0](https://github.com/wrightkit/wright/compare/v0.18.0...v0.19.0) (2026-10-13)


### Features

* **bench:** record language-provider identities in the run identity ([#661](https://github.com/wrightkit/wright/issues/661)) ([4f72644](https://github.com/wrightkit/wright/commit/4f726443cb31ed5791ea4929926689a14d694056))


### Bug Fixes

* **bench:** re-record OPY cli:inspect baselines after provider inspect fix ([#662](https://github.com/wrightkit/wright/issues/662)) ([2903b87](https://github.com/wrightkit/wright/commit/2903b8776d65fd7f882ff9bac1ee4e08566915ac))
* **driver:** load provider-backed inspect through the compile operation ([#659](https://github.com/wrightkit/wright/issues/659)) ([7b7c3f7](https://github.com/wrightkit/wright/commit/7b7c3f77811112e5819657878e992787569443ea)), closes [#655](https://github.com/wrightkit/wright/issues/655)

## [0.18.0](https://github.com/wrightkit/wright/compare/v0.17.1...v0.18.0) (2026-10-13)


### Features

* **benchmark:** support alternative program calls ([#631](https://github.com/wrightkit/wright/issues/631)) ([2bc6055](https://github.com/wrightkit/wright/commit/2bc60559969d0bfaed0b22474fa3ca7736b65a49))


### Bug Fixes

* **analyzer:** resolve callees as calls, not by subroutine name ([#645](https://github.com/wrightkit/wright/issues/645)) ([224cb82](https://github.com/wrightkit/wright/commit/224cb828c21c57f0c3dbeaab4a4e54800ae6c23d))
* **analyzer:** treat disabled statements as not live across analyses ([#644](https://github.com/wrightkit/wright/issues/644)) ([5376f8b](https://github.com/wrightkit/wright/commit/5376f8b65d76ecee63cbf8c2ec8e211f58169b58)), closes [#633](https://github.com/wrightkit/wright/issues/633)
* **cli:** correct wright update exit codes, provenance, and flags ([#652](https://github.com/wrightkit/wright/issues/652)) ([615dd59](https://github.com/wrightkit/wright/commit/615dd59e78bd3af7757530331e2f02d5e69de9bb)), closes [#641](https://github.com/wrightkit/wright/issues/641)
* **cli:** surface invalid UTF-8 and id-less requests on serve transports ([#653](https://github.com/wrightkit/wright/issues/653)) ([85d874a](https://github.com/wrightkit/wright/commit/85d874ace9a6574bafdb62123325bec7aa0b2483)), closes [#642](https://github.com/wrightkit/wright/issues/642)
* **driver:** judge analyze and inspect results on the full finding set ([#646](https://github.com/wrightkit/wright/issues/646)) ([712a752](https://github.com/wrightkit/wright/commit/712a752f1e4502da2233193324ae378be11c0a4b))
* **driver:** reject unknown rule ids in the lint policy ([#651](https://github.com/wrightkit/wright/issues/651)) ([e96526c](https://github.com/wrightkit/wright/commit/e96526c819c4ba43979105b323b3e8ee66fa1aaa)), closes [#640](https://github.com/wrightkit/wright/issues/640)
* **driver:** unify source input handling across discovery and fingerprint ([#647](https://github.com/wrightkit/wright/issues/647)) ([8dde23b](https://github.com/wrightkit/wright/commit/8dde23b793a1911fddd907c30d0114e500451a4d)), closes [#636](https://github.com/wrightkit/wright/issues/636)
* **lpp:** accept capability subsets and retry at the highest mutual version ([#649](https://github.com/wrightkit/wright/issues/649)) ([0509989](https://github.com/wrightkit/wright/commit/050998988f0c2edba210e5ac470c232815dce3eb)), closes [#638](https://github.com/wrightkit/wright/issues/638)
* **lsp:** enforce framing limits and the LSP lifecycle ([#648](https://github.com/wrightkit/wright/issues/648)) ([9331c82](https://github.com/wrightkit/wright/commit/9331c82dce63dcc67f0a7ee65eb012edc7f74e08)), closes [#637](https://github.com/wrightkit/wright/issues/637)
* **release:** align release tooling, CI filters, and metrics check with shipped behavior ([#654](https://github.com/wrightkit/wright/issues/654)) ([0d399fb](https://github.com/wrightkit/wright/commit/0d399fbe83321f6879bf925926f5aa8ec794c088)), closes [#643](https://github.com/wrightkit/wright/issues/643)
* **transform:** keep fold-constants validity- and occurrence-preserving ([#650](https://github.com/wrightkit/wright/issues/650)) ([c0c94c7](https://github.com/wrightkit/wright/commit/c0c94c74ae331b767885e0bff9b4ada856e4d76c)), closes [#639](https://github.com/wrightkit/wright/issues/639)

## [0.17.1](https://github.com/wrightkit/wright/compare/v0.17.0...v0.17.1) (2026-10-11)


### Bug Fixes

* **driver:** drop the unreachable retained-source validation fallback ([#628](https://github.com/wrightkit/wright/issues/628)) ([bb7f496](https://github.com/wrightkit/wright/commit/bb7f496e95ca2a4dad6770c5801541431c1c9a94)), closes [#626](https://github.com/wrightkit/wright/issues/626)

## [0.17.0](https://github.com/wrightkit/wright/compare/v0.16.1...v0.17.0) (2026-10-11)


### Features

* serve structured rule content and grade with program-calls (ADR-0024) ([#625](https://github.com/wrightkit/wright/issues/625)) ([222930a](https://github.com/wrightkit/wright/commit/222930ad007e0d856d5cd0d87eeb1434742c8e2f))

## [0.16.1](https://github.com/wrightkit/wright/compare/v0.16.0...v0.16.1) (2026-10-11)


### Bug Fixes

* **bench:** keep regrade copies faithful and mirror grader availability ([#614](https://github.com/wrightkit/wright/issues/614)) ([#622](https://github.com/wrightkit/wright/issues/622)) ([4ac9183](https://github.com/wrightkit/wright/commit/4ac9183cb6782716fcd0133fdfbf65ad6949da2c))

## [0.16.0](https://github.com/wrightkit/wright/compare/v0.15.4...v0.16.0) (2026-10-11)


### Features

* **bench:** re-grade stored runs when a scenario's checks change ([#621](https://github.com/wrightkit/wright/issues/621)) ([b77f80a](https://github.com/wrightkit/wright/commit/b77f80aa8f151f20698f67dbef5ae5be38939732))


### Bug Fixes

* **driver:** run canonical validation on check for compile-capable providers ([#619](https://github.com/wrightkit/wright/issues/619)) ([3ec316d](https://github.com/wrightkit/wright/commit/3ec316df5bdcd66f9545276e6c6502a892ddedf4))

## [0.15.4](https://github.com/wrightkit/wright/compare/v0.15.3...v0.15.4) (2026-10-10)


### Bug Fixes

* **bench:** grade greenfield requirements on canonical output, accepting equivalent forms ([#611](https://github.com/wrightkit/wright/issues/611)) ([5064df5](https://github.com/wrightkit/wright/commit/5064df5186e541cfe341b011c23bf22d25f5e84c))
* **bench:** retry a provider interruption after 60s before the --limits-poll wait ([#612](https://github.com/wrightkit/wright/issues/612)) ([a7d5b3e](https://github.com/wrightkit/wright/commit/a7d5b3ee8c72eecad7ec9dbb527f9fd63560f35c))

## [0.15.3](https://github.com/wrightkit/wright/compare/v0.15.2...v0.15.3) (2026-10-10)


### Bug Fixes

* **bench:** warm the agent session before measuring and accept provider drift ([#608](https://github.com/wrightkit/wright/issues/608)) ([87cce04](https://github.com/wrightkit/wright/commit/87cce0412ca2ccdb1ae984c2bbfaa6cbe4090005))

## [0.15.2](https://github.com/wrightkit/wright/compare/v0.15.1...v0.15.2) (2026-10-10)


### Bug Fixes

* **bench:** run the network canary on the host PATH ([#605](https://github.com/wrightkit/wright/issues/605)) ([16b0931](https://github.com/wrightkit/wright/commit/16b09313a5a9cb247c1a7ab4b7282e99859e3aca)), closes [#590](https://github.com/wrightkit/wright/issues/590)
* **driver:** validate and apply lint fixes through the OPY provider ([#602](https://github.com/wrightkit/wright/issues/602)) ([96f80c8](https://github.com/wrightkit/wright/commit/96f80c8b715882074ede59e1e46d279ec5c4950e))


### Dependencies

* **agent:** sync vendored guide to skills@09727d7 ([#607](https://github.com/wrightkit/wright/issues/607)) ([c868b42](https://github.com/wrightkit/wright/commit/c868b4209bd7578a1532f04abece1603aac068b4)), closes [#539](https://github.com/wrightkit/wright/issues/539)

## [0.15.1](https://github.com/wrightkit/wright/compare/v0.15.0...v0.15.1) (2026-10-09)


### Bug Fixes

* **driver+cli:** service workflows without snapshot prewarm; validated and configured serve startup ([#596](https://github.com/wrightkit/wright/issues/596)) ([78a3a0e](https://github.com/wrightkit/wright/commit/78a3a0eb5e7d6e2d80fbf51604ed281e33589d9d))
* **driver:** run service check/compile without a canonical snapshot prewarm ([#599](https://github.com/wrightkit/wright/issues/599)) ([8317ca1](https://github.com/wrightkit/wright/commit/8317ca1ec0b1edbfa68a50b3ac31d365c430f885))

## [0.15.0](https://github.com/wrightkit/wright/compare/v0.14.0...v0.15.0) (2026-10-09)


### Features

* **bench:** --wait-for-limits on evaluate and suite waits out provider limits ([#586](https://github.com/wrightkit/wright/issues/586)) ([6aff2ea](https://github.com/wrightkit/wright/commit/6aff2ea0354580943b1e280eccd22177d44b41c3))
* **bench:** lift cell set; a combined report no longer overwrites a run's own ([#588](https://github.com/wrightkit/wright/issues/588)) ([4b8ed30](https://github.com/wrightkit/wright/commit/4b8ed30d63070e721ae3f573cb66cb1ecf8d983e))


### Bug Fixes

* **analyzer:** locate findings at the nearest recorded span ([#578](https://github.com/wrightkit/wright/issues/578)) ([21fdc5e](https://github.com/wrightkit/wright/commit/21fdc5e268e059b81709007135ee4f5598b434ad))
* **bench:** a run killed at the time limit keeps the model and effort it was launched with ([#587](https://github.com/wrightkit/wright/issues/587)) ([8a115b0](https://github.com/wrightkit/wright/commit/8a115b041709323f4f7b5ceffc3f145646a05c37))
* **bench:** block package managers under network off and reject runs that fetch a withheld tool ([#585](https://github.com/wrightkit/wright/issues/585)) ([d89c804](https://github.com/wrightkit/wright/commit/d89c8044d31dfb71255025650e1f3967bd120cd2))
* **driver:** classify provider edit failures by refusal vs execution fault ([#570](https://github.com/wrightkit/wright/issues/570)) ([#582](https://github.com/wrightkit/wright/issues/582)) ([f1d00d3](https://github.com/wrightkit/wright/commit/f1d00d36e1fb9fe1e567eae267fb0e52d95aa44f))
* **driver:** propagate provider capability-query failure in compile ([#580](https://github.com/wrightkit/wright/issues/580)) ([0454d02](https://github.com/wrightkit/wright/commit/0454d024c3c61aa190c025077ae6c8b97d3a019f)), closes [#571](https://github.com/wrightkit/wright/issues/571)
* **serve:** carry each rule's event binding in rules results ([#579](https://github.com/wrightkit/wright/issues/579)) ([8badb6a](https://github.com/wrightkit/wright/commit/8badb6a2bc627579d1fc61079382a88626c0e608))
* **serve:** name the accepted shapes in untagged Address refusals ([#581](https://github.com/wrightkit/wright/issues/581)) ([b218443](https://github.com/wrightkit/wright/commit/b21844347b6b026d452f72705335a7b3a5d9ccb3))


### Dependencies

* move workshop-rs to 1.10.1 and render raw-setting suggestions ([#589](https://github.com/wrightkit/wright/issues/589)) ([1c21fea](https://github.com/wrightkit/wright/commit/1c21feaa9baeed94f3cedb095c775d33df6dbde3))

## [0.14.0](https://github.com/wrightkit/wright/compare/v0.13.0...v0.14.0) (2026-10-08)


### Features

* **analyzer:** refuse evaluate-once only inside reevaluated parameters ([#577](https://github.com/wrightkit/wright/issues/577)) ([4c6f3d4](https://github.com/wrightkit/wright/commit/4c6f3d47d567c0b85284eca1717316ed2a326f31))


### Bug Fixes

* **analyzer:** name the nearest declared names on unknown-symbol and unknown-rule ([#574](https://github.com/wrightkit/wright/issues/574)) ([ab5c71a](https://github.com/wrightkit/wright/commit/ab5c71aa684d11b01232e337a85c241a6fd8492d))
* **driver:** preserve typed provider failure classification at the source-provider boundary ([#569](https://github.com/wrightkit/wright/issues/569)) ([#575](https://github.com/wrightkit/wright/issues/575)) ([9ef33f2](https://github.com/wrightkit/wright/commit/9ef33f21d3ecfe8bbb76a3b67223903d3e5b2b50))
* **serve:** accept every emitted entry kind in lookup's kind filter ([#573](https://github.com/wrightkit/wright/issues/573)) ([5ad308c](https://github.com/wrightkit/wright/commit/5ad308c2d64738cd346e2d8074d51b7af42dd6d4))
* **serve:** resolve rules span paths in every selection form ([#572](https://github.com/wrightkit/wright/issues/572)) ([574f738](https://github.com/wrightkit/wright/commit/574f7385549cf9a5f42e427182ae071c15813b05))

## [0.13.0](https://github.com/wrightkit/wright/compare/v0.12.0...v0.13.0) (2026-10-08)


### Features

* **serve:** expose the approved read operations over MCP ([#566](https://github.com/wrightkit/wright/issues/566)) ([4f8a1d5](https://github.com/wrightkit/wright/commit/4f8a1d59ae8706311823e34dcd7be88ab5dcda3b))


### Bug Fixes

* **cli:** keep wright lookup from blocking on terminal stdin ([#568](https://github.com/wrightkit/wright/issues/568)) ([41011a9](https://github.com/wrightkit/wright/commit/41011a9f3bce5a86531a5cac82fd27a1ed651877))
* **serve:** name the accepted values in closed-vocabulary selection errors ([#564](https://github.com/wrightkit/wright/issues/564)) ([225a809](https://github.com/wrightkit/wright/commit/225a809b9ef85ca50ee66936b1e0eed9b338c723))

## [0.12.0](https://github.com/wrightkit/wright/compare/v0.11.0...v0.12.0) (2026-10-08)


### Features

* **agent:** default provider-op document sets from the loaded session ([#561](https://github.com/wrightkit/wright/issues/561)) ([b1af278](https://github.com/wrightkit/wright/commit/b1af278ff42bceeaf7ddeb04ee9e8ed197dcff56))
* **bench:** run the per-release agent tracking set ([#559](https://github.com/wrightkit/wright/issues/559)) ([73453b4](https://github.com/wrightkit/wright/commit/73453b412762a6c4943509bd6b38f74a15ad0726))
* **lint:** validated automated fixes for exact findings ([#560](https://github.com/wrightkit/wright/issues/560)) ([25fe9e5](https://github.com/wrightkit/wright/commit/25fe9e5dda5cf23d7fdc881270c36e107d901cab))
* **lsp:** back raw Workshop diagnostics and semantic queries ([#558](https://github.com/wrightkit/wright/issues/558)) ([bf0fb38](https://github.com/wrightkit/wright/commit/bf0fb388808bd5cf641bf286b463fd721f17bb46))


### Bug Fixes

* **agent:** a stray .txt no longer claims Workshop ownership of a directory ([#552](https://github.com/wrightkit/wright/issues/552)) ([093a242](https://github.com/wrightkit/wright/commit/093a242a912e17c02ccb11d2215a03d2254340aa))
* **agent:** program.files counts the loaded source files on provider projects ([#546](https://github.com/wrightkit/wright/issues/546)) ([d40f6d1](https://github.com/wrightkit/wright/commit/d40f6d1d0f99b291ce4ecb6ad10f6bf04597d702))


### Performance Improvements

* **ci:** split metrics drift from benchmarks and cut redundant metrics runs ([#544](https://github.com/wrightkit/wright/issues/544)) ([d48977f](https://github.com/wrightkit/wright/commit/d48977f8421be5e31e76de3e3fa5a5ca0692617f))

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
