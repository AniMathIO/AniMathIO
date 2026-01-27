# Changelog

## [1.6.0](https://github.com/AniMathIO/AniMathIO/compare/v1.5.0...v1.6.0) (2026-01-27)


### Features

* add mocks for canvas and process in test setup ([34ec3e9](https://github.com/AniMathIO/AniMathIO/commit/34ec3e9e520e6ab3c58e7c7d05d3e0d9f5a6dc77))
* added Definition of Done (DOD) document ([d3bfcd3](https://github.com/AniMathIO/AniMathIO/commit/d3bfcd3aa09c411e131f546746ede4ac0faf3590))
* added desktop and Flatpak metadata to build configs ([b3dffce](https://github.com/AniMathIO/AniMathIO/commit/b3dffce1fec0735eab5d9d4bb16bd7eb0ab7c2de))
* Added file association and 'Open with' support for .animathio ([5f2e5c7](https://github.com/AniMathIO/AniMathIO/commit/5f2e5c7bd87f2406ba660ab7f7fb55acd09a51a7))
* added project loading modal with progress and error states ([182bdb8](https://github.com/AniMathIO/AniMathIO/commit/182bdb81a6f6975ad3bba5daa1d1e9ccbcb19bee))
* dashboard page ([5c5e6f4](https://github.com/AniMathIO/AniMathIO/commit/5c5e6f4f60a877a99eaa8abf322c9c7c0c4457ee))
* **dashboard:** Added external URL handler and dashboard links ([4e7cfd9](https://github.com/AniMathIO/AniMathIO/commit/4e7cfd923d3bd435c075c6d7474a72a731900d34))
* dependency update ([9ad4ae8](https://github.com/AniMathIO/AniMathIO/commit/9ad4ae8dbee8b3a4583ccd1b97884584bfcd3815))
* Improved selected element handling in State class ([4a4b389](https://github.com/AniMathIO/AniMathIO/commit/4a4b38988f8a73a03e7eee6189deecaa5525f059))
* locked package versions ([8048bc6](https://github.com/AniMathIO/AniMathIO/commit/8048bc6303ecca146665da06d334f531831b251c))
* simplified electron-builder file filters and enable asar ([782321d](https://github.com/AniMathIO/AniMathIO/commit/782321d26d17d9a1719efb29060dccdf42efd699))
* **tests:** enhanced testing workflow with unit and browser tests ([9aad4ec](https://github.com/AniMathIO/AniMathIO/commit/9aad4ece288330a7f6f4cd7fb62be65288922f62))
* **text2latex:** added Gemini model selection and dynamic model fetching ([a15b2e5](https://github.com/AniMathIO/AniMathIO/commit/a15b2e5381b527c33d0463e9a53f29ac12a7730b))
* update from staging ([95a1733](https://github.com/AniMathIO/AniMathIO/commit/95a1733479fd425d78076b11a7c275e02d4350d2))
* update from staging ([2b9450f](https://github.com/AniMathIO/AniMathIO/commit/2b9450fe8a2e87d8aab24196443bb5a6b329beaf))
* update from staging ([661c2ad](https://github.com/AniMathIO/AniMathIO/commit/661c2adedd5fa51d3aaedd03c6a4adc6c80d35f2))
* updated SECURITY.md with project status and version support ([3b8ee48](https://github.com/AniMathIO/AniMathIO/commit/3b8ee48c7f99a1bf2fa17289efa3c43e9f3b25e1))
* WIP Added Flatpak support to build configs and scripts ([6a1c151](https://github.com/AniMathIO/AniMathIO/commit/6a1c151b77b52fc88152634d16d060e539039a2d))
* WIP dashboard ([61b9938](https://github.com/AniMathIO/AniMathIO/commit/61b99386626c6fca4a6935d9c13b1bd2b11ad377))


### Bug Fixes

* added script to fix Tailwind oxide packaging issues ([07fd1f5](https://github.com/AniMathIO/AniMathIO/commit/07fd1f54ca7b06cc6f7639a45173451b1600c83d))
* moved theme mode loading to _app.tsx for early application ([0648139](https://github.com/AniMathIO/AniMathIO/commit/0648139c40f0fe88860504299e15a8ba0da7795b))
* Node.js version setup in CI workflow ([1205a7d](https://github.com/AniMathIO/AniMathIO/commit/1205a7d9735783df4e6ca2b0b848a9421291005c))
* **tests:** fixed ui tests ([3ec7d07](https://github.com/AniMathIO/AniMathIO/commit/3ec7d0726059ccf4ee2ac2678ac15dc28b7ecef0))
* **tests:** updated coverage thresholds and improved tests ([4df17c3](https://github.com/AniMathIO/AniMathIO/commit/4df17c31d089d5dfab05c6cb3f93149f15d81a5b))
* **tests:** WIP working on tests ([92e5216](https://github.com/AniMathIO/AniMathIO/commit/92e5216c203add88720bcd1da772454f7b2fc4ca))

## [1.5.0](https://github.com/AniMathIO/AniMathIO/compare/v1.4.1...v1.5.0) (2025-03-17)


### ⚠ BREAKING CHANGES

* fixed rendering state bug in mathematical elements
* added backwards compatible audio settings state for project saving and loading
* added missing dependencies to ci test runner
* updated packages and locked version to resolve install and build errors

### Features

* added space hotkey ([5630b68](https://github.com/AniMathIO/AniMathIO/commit/5630b688dde26d25db476e1f7345673abf7e6269))
* added vitest ui package for better testing experience ([ebdc423](https://github.com/AniMathIO/AniMathIO/commit/ebdc423de73145634185dd73adac2a41caeea872))
* improve test coverage ([605d1e4](https://github.com/AniMathIO/AniMathIO/commit/605d1e47b825d5e20705c57aa3e3fba0c440fc07))
* installed end configured vitest ([7776d5e](https://github.com/AniMathIO/AniMathIO/commit/7776d5e840f722a6e488d315e851117d15e485ca))
* updated help submenu links ([fee61f8](https://github.com/AniMathIO/AniMathIO/commit/fee61f808eca71f40aa60d09244743340b942807))
* WIP audio mixer panel ([60e27cf](https://github.com/AniMathIO/AniMathIO/commit/60e27cf045d5dd57a126beac6ca281f1b8cb8638))
* WIP working on audio recorder component ([2b522a5](https://github.com/AniMathIO/AniMathIO/commit/2b522a54692def0c9d26811e044fd42097895bc6))
* WIP working on audio recorder component, fixed light mode audio styling ([596ea0e](https://github.com/AniMathIO/AniMathIO/commit/596ea0e24b4c59638e330c923c3604b241ab218d))
* working on automated tests ([ada56ea](https://github.com/AniMathIO/AniMathIO/commit/ada56ea2779f07cc58b6917f703920d43877a2d2))
* working on state related unit tests ([d6face3](https://github.com/AniMathIO/AniMathIO/commit/d6face390f21f031bdc0cc604ad94c83cdd4a89f))


### Bug Fixes

* added backwards compatible audio settings state for project saving and loading ([368d7b8](https://github.com/AniMathIO/AniMathIO/commit/368d7b8102721c7d64ee7076269ba78bdee1f6ed))
* added missing dependencies to ci test runner ([5f9721a](https://github.com/AniMathIO/AniMathIO/commit/5f9721a67380f1d176b276df2446d2db25d2fdc8))
* adding missing dependencies to ci runner ([a126d5e](https://github.com/AniMathIO/AniMathIO/commit/a126d5e961e19270de45e3137194ccd9d036c2fb))
* correcting node version for automated tests ([ed6f252](https://github.com/AniMathIO/AniMathIO/commit/ed6f25246086a13d1155575643e7583b5953727a))
* correcting node version for automated tests ([7bd3bc1](https://github.com/AniMathIO/AniMathIO/commit/7bd3bc1d35e1df71e537b85081aef77a423bf277))
* correcting node version for automated tests ([4bc0f34](https://github.com/AniMathIO/AniMathIO/commit/4bc0f34920e9c4cdcc8934444d73ab187ce521eb))
* correcting node version for automated tests ([93598aa](https://github.com/AniMathIO/AniMathIO/commit/93598aa24d6c56b52b836ab31f4d8121fbcfadd0))
* fixed  CJS Node API deprecated error ([39ae1c7](https://github.com/AniMathIO/AniMathIO/commit/39ae1c74fcbace7ee7e0e7c1b82cb821143553c3))
* fixed build error ([994b3ab](https://github.com/AniMathIO/AniMathIO/commit/994b3abc9901bbd8f1a313c92e764b8024b0ed76))
* fixed rendering state bug in mathematical elements ([9f4be2f](https://github.com/AniMathIO/AniMathIO/commit/9f4be2f598f85424db4f2f8801d1921773bc604f))
* improve test runner ci with dynamic node version loading ([c367e34](https://github.com/AniMathIO/AniMathIO/commit/c367e34c3afbccccb3042cf7650b05948dee2fa8))
* infinite and nan time in audio resource panel for recorded audio ([7b62958](https://github.com/AniMathIO/AniMathIO/commit/7b62958fcc4df47e1b7c0289479f84eadf35cd79))
* node-gyp build for automated tests ([2a7cb19](https://github.com/AniMathIO/AniMathIO/commit/2a7cb19680fe174225ba505c49590e0567940af5))
* refactored and fixed weird test cases ([a24458b](https://github.com/AniMathIO/AniMathIO/commit/a24458bbe832af134e85876978f04c5f79e56b45))
* test runs ([185d22b](https://github.com/AniMathIO/AniMathIO/commit/185d22b5f40f7aeae680deee8d37b9a50e478fc0))
* updated packages and locked version to resolve install and build errors ([d418e28](https://github.com/AniMathIO/AniMathIO/commit/d418e28ac1767eec494dce5eb47836ec6ba958ed))
* WIP fixed rerendering bug when audio volume changed ([318f828](https://github.com/AniMathIO/AniMathIO/commit/318f828ca93afaf0a12b3c00ea7537e56e8c8cdf))
* working on automated test fix ([7258205](https://github.com/AniMathIO/AniMathIO/commit/725820592e71e9e4eb3e1610913754977d9ce099))
* working on automated tests ([0a6d893](https://github.com/AniMathIO/AniMathIO/commit/0a6d893c6247fd4baa8ffc3888286afe3d41b73f))
* working on dependency errors in tests ci runner ([af885c7](https://github.com/AniMathIO/AniMathIO/commit/af885c7f9feec845065ad715c40cbe03530d18ff))
* working on fixing automated tests ([309b47d](https://github.com/AniMathIO/AniMathIO/commit/309b47d405277a03409739899a06c7d0daa88de2))
* working on test fixes ([b142daf](https://github.com/AniMathIO/AniMathIO/commit/b142daf25c0cd1ba206e974c3ef9610199faf73b))


### Miscellaneous Chores

* release 1.5.0 ([3573bad](https://github.com/AniMathIO/AniMathIO/commit/3573badeebb26bf4798a3f09c16772c12f8d1d86))

## [1.4.1](https://github.com/AniMathIO/AniMathIO/compare/v1.4.0...v1.4.1) (2025-03-09)


### Features

* fixed [#84](https://github.com/AniMathIO/AniMathIO/issues/84) bug ([5907ac8](https://github.com/AniMathIO/AniMathIO/commit/5907ac881e9dc3a1f279779112dc025d9feb2026))
* fixed [#84](https://github.com/AniMathIO/AniMathIO/issues/84) bug ([4c9dee2](https://github.com/AniMathIO/AniMathIO/commit/4c9dee242b6c79c22de7b16e20af45e17b6204ea))
* update from main ([7530ffc](https://github.com/AniMathIO/AniMathIO/commit/7530ffc3d4810851d45ab827edc736ec8d2a3d57))


### Miscellaneous Chores

* release 1.4.1 ([8ef26b7](https://github.com/AniMathIO/AniMathIO/commit/8ef26b7f44b94d1e4bf0385616ec31a5c677ac59))

## [1.4.0](https://github.com/AniMathIO/AniMathIO/compare/v1.3.1...v1.4.0) (2025-03-08)


### Features

* improved project saving and loading with compression ([1f98c63](https://github.com/AniMathIO/AniMathIO/commit/1f98c631d9942a968b6f877297565f2d51fc96f5))
* improved text-to-latex conversion ui ([07c0c56](https://github.com/AniMathIO/AniMathIO/commit/07c0c56721d24fb2f432abb380a259595d9dea25))
* integrated smart background removal for images ([26b5681](https://github.com/AniMathIO/AniMathIO/commit/26b568197a6dc187bac7e8d8beced86940c6c968))
* update from main ([3f35cf2](https://github.com/AniMathIO/AniMathIO/commit/3f35cf253ac66947df1e469b732081600f810efe))
* update from main ([999b6cd](https://github.com/AniMathIO/AniMathIO/commit/999b6cd4ba5d06b83ab6e4ae06346907409b7978))
* WIP implemented save and load modals ([00aee67](https://github.com/AniMathIO/AniMathIO/commit/00aee675a5aea72399b99f567d61fab24e498bc5))
* WIP working on project saving and loading ([2f42285](https://github.com/AniMathIO/AniMathIO/commit/2f42285a6d5aa44bae048b5453b2f8fec12c1ce9))
* WIP working on proper project saving and loading ([0f4515c](https://github.com/AniMathIO/AniMathIO/commit/0f4515c6c2d96c44348c8dc1d74a93b06c4d07c7))
* working on file saving and loading dialogs ([46dcf6f](https://github.com/AniMathIO/AniMathIO/commit/46dcf6f3a6c995214fdd866b4ad2b3dd5b2b9d75))
* working on text-to-latex conversion with GeminiAPI ([7923981](https://github.com/AniMathIO/AniMathIO/commit/79239812a51d3fc8dfde2e86661b75c3b0928e52))


### Bug Fixes

* fixed double popup bug when save dialog closed ([68f6d08](https://github.com/AniMathIO/AniMathIO/commit/68f6d08ca5e026dddf32a8565c89a3d7f545de4f))
* fixed type error in state ([1df9f9c](https://github.com/AniMathIO/AniMathIO/commit/1df9f9cd38880829a10172f586752f560bc87d1c))
* the images, audio, and video are not loaded to the editor elements correctly ([4400c56](https://github.com/AniMathIO/AniMathIO/commit/4400c563484aa5bba8abac176f4d68d7660b2e05))


### Miscellaneous Chores

* release 1.4.0 ([7cfcc8e](https://github.com/AniMathIO/AniMathIO/commit/7cfcc8e714447801811ac789eee69c1c7874e42b))

## [1.3.1](https://github.com/AniMathIO/AniMathIO/compare/v1.3.0...v1.3.1) (2025-03-01)


### ⚠ BREAKING CHANGES

* correcting the bugs caused by the shortcuts

### Features

* updated node version and packages ([8dd7538](https://github.com/AniMathIO/AniMathIO/commit/8dd7538fbe488b3ac9815005e8aa152f3faf95e3))


### Bug Fixes

* correcting the bugs caused by the shortcuts ([5e664d6](https://github.com/AniMathIO/AniMathIO/commit/5e664d6c589459f836241db2db92225bfeef5f99))


### Miscellaneous Chores

* release 1.0.3 ([e2941d6](https://github.com/AniMathIO/AniMathIO/commit/e2941d6946749543018c13cbf16ec2eaac304fbd))
* release 1.3.1 ([58663cd](https://github.com/AniMathIO/AniMathIO/commit/58663cd859fec0c3a7de0586d1ebd4e3354b014c))

## [1.3.0](https://github.com/AniMathIO/AniMathIO/compare/v1.2.0...v1.3.0) (2024-11-25)


### Features

* added missing package dependency for macos build ([492033a](https://github.com/AniMathIO/AniMathIO/commit/492033a5f0721775f17a0c85c3d4975c3aaa7b59))
* implemented basic guidelines ([b696001](https://github.com/AniMathIO/AniMathIO/commit/b696001ab7bef67b09f716bcac65ef318718d751))
* implemented basic keyboard shortcuts ([f6c48b9](https://github.com/AniMathIO/AniMathIO/commit/f6c48b9b71be8e4716d2a792b880ab8b4f555434))
* implemented better multimedia controls ([2ccf431](https://github.com/AniMathIO/AniMathIO/commit/2ccf43176859b14d33890c777fd5ed2d9119c979))
* metadata update v1.3.0 ([2a9800d](https://github.com/AniMathIO/AniMathIO/commit/2a9800d7bf95655ffafef2605039dcb71337e749))
* README.md update ([7f20f9b](https://github.com/AniMathIO/AniMathIO/commit/7f20f9b188e7b4a94663c82202423d152a2d7b9f))
* README.md update about os support ([0668383](https://github.com/AniMathIO/AniMathIO/commit/0668383fd8b1e7fcbeff894dffcc9fc148451aa6))
* update node version ([495afaa](https://github.com/AniMathIO/AniMathIO/commit/495afaa6d5222f5011a09f7fe49d9890763d2165))
* update README.md with star history ([1ccb475](https://github.com/AniMathIO/AniMathIO/commit/1ccb47515e8891ce2841216cfca4aaacf4b76c82))
* updated package dependencies ([dfd9cc3](https://github.com/AniMathIO/AniMathIO/commit/dfd9cc3c9b3b6dcb8a7559d80db5f5c74c0d6804))
* updateREADME.md removed multiple star history title ([2708500](https://github.com/AniMathIO/AniMathIO/commit/2708500272b906aee9a654c2fced866719d5e112))
* updating project dependencies ([73b6651](https://github.com/AniMathIO/AniMathIO/commit/73b6651de04b0fe8b36b30cd60bd8aae1aa4d876))


### Bug Fixes

* fixed macos missing menubar bug ([58a81d5](https://github.com/AniMathIO/AniMathIO/commit/58a81d5ee99831e6aae620b2ab93aa38e15df9df))

## [1.2.0](https://github.com/AniMathIO/AniMathIO/compare/v1.1.0...v1.2.0) (2024-06-15)


### Features

* **app:** added visual corrections to rendering modal ([59d55d6](https://github.com/AniMathIO/AniMathIO/commit/59d55d6244e44095abba2524f9a0471d3a4cceab))
* **app:** implemented dark mode in the components ([463f461](https://github.com/AniMathIO/AniMathIO/commit/463f4610843448f88c188c31516c87d1ebfebb87))
* **app:** implemented default scaling for certain aspect ratios ([0ced7c7](https://github.com/AniMathIO/AniMathIO/commit/0ced7c7e33f2f6de4abf460760953868079facbb))
* **app:** implemented separate audio track for video inputs ([ff811b2](https://github.com/AniMathIO/AniMathIO/commit/ff811b2c9ce27ce0e3ff68b4e649d8652d94f796))
* **app:** made dark/light mode persistent ([6227958](https://github.com/AniMathIO/AniMathIO/commit/622795837bc6a33a2d8d652d33b75639f3c5d06e))
* **app:** updated packages ([cddafcf](https://github.com/AniMathIO/AniMathIO/commit/cddafcf02d790340f5073f94ff5b229be5b964b0))
* **app:** working on basic settings modal ([23a8ae3](https://github.com/AniMathIO/AniMathIO/commit/23a8ae3d9f67b08609a0a19135502fb69fb32143))
* **app:** working on canvas scaling ([77d4dec](https://github.com/AniMathIO/AniMathIO/commit/77d4decf592d10bebe7d09e5478f07b18e0f2cb7))
* **app:** working on scaling ([2584ae8](https://github.com/AniMathIO/AniMathIO/commit/2584ae82f67f7d0bc4007f6d4ca11633572325f9))
* **app:** working on settings modal ([279eb2a](https://github.com/AniMathIO/AniMathIO/commit/279eb2aca2b17bf0f29ab87dae25edfdb7753bdc))
* package version update v1.2.0 ([bf38960](https://github.com/AniMathIO/AniMathIO/commit/bf3896035cfe414d86f6408585f7468f94053518))


### Bug Fixes

* **app:** [#38](https://github.com/AniMathIO/AniMathIO/issues/38) ([8e9e0d8](https://github.com/AniMathIO/AniMathIO/commit/8e9e0d8e4cf06d384da6f89d3c2e80177d40126e))
* **app:** fixed app typo ([7e2394d](https://github.com/AniMathIO/AniMathIO/commit/7e2394db13b223733829ca7f471b95e9c30ba9f8))
* **app:** fixed mp4 rendering ([e3dc0c5](https://github.com/AniMathIO/AniMathIO/commit/e3dc0c52e6c9aee91a432db91a9739006d12004e))

## [1.1.0](https://github.com/AniMathIO/AniMathIO/compare/v1.0.0...v1.1.0) (2024-05-20)


### Features

* package version update v1.1.0 ([e99f203](https://github.com/AniMathIO/AniMathIO/commit/e99f203d817d7d5afacd81748ce00ee59a79533e))
* **v1.0.0:** updated readme v1.0.0 ([9e2a3ab](https://github.com/AniMathIO/AniMathIO/commit/9e2a3abcdada1be67161b57cdd61ff72ae5cd5a1))


### Bug Fixes

* configurations for electron builds ([46ab1a7](https://github.com/AniMathIO/AniMathIO/commit/46ab1a7d3f58b71c8f3a9bc1d6e6ea628ddf7698))

## 1.0.0 (2024-05-20)


### ⚠ BREAKING CHANGES

* fixed mafs element addition errors
* working on mafs integration
* working on proper mafs element additions
* fixed windows build errors :)
* working on fixing windows build, had to downgrade nextjs
* project structure refactoring for windows build
* working on windows build errors
* **app:** implemented working mafs integration
* working on manim element addition to canvas
* working on element rendering to canvas
* **app:** fixed rendering when no audio is set
* **app:** fixed multiple rendering error but multiple audio doesnt works properly
* **app:** rendering errors

### Features

* add LICENSE ([518a477](https://github.com/AniMathIO/AniMathIO/commit/518a477e129c4871ac4c39b84781c73e5686ccc8))
* added additional element type icon ([14b5646](https://github.com/AniMathIO/AniMathIO/commit/14b564635a9a562e15db3ba09a8b59fb47c0bf43))
* added aliases for higher level elements ([3546a57](https://github.com/AniMathIO/AniMathIO/commit/3546a57621556511bd71cdd861c0f2085c585962))
* added custom LaTex element ([a993966](https://github.com/AniMathIO/AniMathIO/commit/a993966168380ca6c77932bfbfa6197314687014))
* added disclaimer to plot functions ([ed750f4](https://github.com/AniMathIO/AniMathIO/commit/ed750f43354be6e888d6f67c967b97e41a27346d))
* added error handling alerts ([797ea52](https://github.com/AniMathIO/AniMathIO/commit/797ea528b47f0c4dc6e999906f6d49ace01bf820))
* added new mafs elements ([a9a3c87](https://github.com/AniMathIO/AniMathIO/commit/a9a3c87f051f8bfa7186608c56cff594fcebf118))
* added package updates ([422371a](https://github.com/AniMathIO/AniMathIO/commit/422371abfa7185a760529edafb63ffe0bcb17d95))
* added restrictions related to size of the mafs element ([93ccf2e](https://github.com/AniMathIO/AniMathIO/commit/93ccf2e070c6cc1295d7489793083dcdade47e1a))
* **app:** added .nvmrc ([864d55c](https://github.com/AniMathIO/AniMathIO/commit/864d55ccca1bb722f4d2627c661a4fa09f4c6451))
* **app:** added new menu option type ([5348ff8](https://github.com/AniMathIO/AniMathIO/commit/5348ff83832b2d8ab6cb3678e231040ac9625a32))
* **app:** called new menu option when clicked ([ede230b](https://github.com/AniMathIO/AniMathIO/commit/ede230b5823a69bf238fd7d013258026da4d4c61))
* **app:** created new menu option ([7c1d796](https://github.com/AniMathIO/AniMathIO/commit/7c1d79678e96062bd29d10f615dba6b5625fcaf0))
* **app:** debugging missing svg element from canvas ([bf76abc](https://github.com/AniMathIO/AniMathIO/commit/bf76abcf88919cc012aab7da9a724635985d9980))
* **app:** downgraded nextjs version and configured packages ([71164f3](https://github.com/AniMathIO/AniMathIO/commit/71164f390c16f91a5aaae7c9c70910a12801ae13))
* **app:** fixed multiple audio rendering ([967dc62](https://github.com/AniMathIO/AniMathIO/commit/967dc625e337531b552abd7ae3b1fea6e3a69699))
* **app:** implemented base menu optional panel for mathematical objects ([57108f9](https://github.com/AniMathIO/AniMathIO/commit/57108f9e78bdf26cfbfc5f698de6cc2e675f2b38))
* **app:** implemented fabric utilities ([7dbb2be](https://github.com/AniMathIO/AniMathIO/commit/7dbb2be3729c6bf6a505a8e833cd37898ae02fe7))
* **app:** implemented shared partial button ([1558217](https://github.com/AniMathIO/AniMathIO/commit/155821752ac759eaf456c3d94ace1450d2d2230e))
* **app:** implemented sidepanel ([b84aef4](https://github.com/AniMathIO/AniMathIO/commit/b84aef470cbf2c4acc1cc4db1c59c35c4236437e))
* **app:** implemented state manager ([11628c2](https://github.com/AniMathIO/AniMathIO/commit/11628c201d9f0ae49c71e372ce3c9c7b25b47688))
* **app:** implemented state types ([1cecaa1](https://github.com/AniMathIO/AniMathIO/commit/1cecaa1afc44a80e4ad637ddb8496584fe8cf434))
* **app:** implemented timeline ([025f6a1](https://github.com/AniMathIO/AniMathIO/commit/025f6a1d8702f992b31f691b89f693e33399f711))
* **app:** implemented working mafs integration ([ac8e7bc](https://github.com/AniMathIO/AniMathIO/commit/ac8e7bcd818792c118324e5906fcd26e569773e3))
* **app:** installed dependencies ([9fbc121](https://github.com/AniMathIO/AniMathIO/commit/9fbc12165f7299e06140aeef1e14b5f073944757))
* **app:** installed package dependencies ([9722550](https://github.com/AniMathIO/AniMathIO/commit/9722550907846f04b9cb6b36a4447dbf46e130e1))
* **app:** rebranding ([b8885cf](https://github.com/AniMathIO/AniMathIO/commit/b8885cf3a70d3a43277df685ec49267f23b036b7))
* **app:** rebranding; replacing logo ([0875fed](https://github.com/AniMathIO/AniMathIO/commit/0875fed1227d4b1c5afec81a0e86132561d21702))
* **app:** refactored styles ([e1f9823](https://github.com/AniMathIO/AniMathIO/commit/e1f98233643b64578a8e4c9905a18e31d570c7d1))
* **app:** refactored timeline bugs, implemented missing functionalities ([f39f9ab](https://github.com/AniMathIO/AniMathIO/commit/f39f9abfed4f26d60228b5b560b9d255fc7a8022))
* **app:** updated default resolution based on current render ([93ce8ba](https://github.com/AniMathIO/AniMathIO/commit/93ce8bab2a7d33d90edc662df8a8f13fefa56e61))
* **app:** updated electron version and installed required dependencies ([99b56c2](https://github.com/AniMathIO/AniMathIO/commit/99b56c28fcaac502ce653d854b69bd7ee954eee5))
* **app:** updated nextjs version and changed outdated component calls ([3bfd2be](https://github.com/AniMathIO/AniMathIO/commit/3bfd2be122f59dfbfc62b8f01052fcc04b4adcdf))
* **app:** updated resources ([8ee7665](https://github.com/AniMathIO/AniMathIO/commit/8ee76659154fd33fa67ec9eb2e1b19cec19c7b33))
* **app:** working on mafs element wrapping and showing as resource in the panels ([72629f0](https://github.com/AniMathIO/AniMathIO/commit/72629f081159180588bc5bd9e6c711edc3a9df94))
* **app:** working on mathematical element rendering on canvas ([8e6d3a3](https://github.com/AniMathIO/AniMathIO/commit/8e6d3a35b595c1806f3845e63643b3c43b3a9e8f))
* Create SECURITY.md ([eec0f56](https://github.com/AniMathIO/AniMathIO/commit/eec0f561fa941f51f296b0e55988cd3b382421d9))
* created additional element type ([3b3b8fc](https://github.com/AniMathIO/AniMathIO/commit/3b3b8fc966187fe573c7702ef0ea4278a58b19e2))
* **docs:** readme update about the dependencies for building the app ([4a6c8ef](https://github.com/AniMathIO/AniMathIO/commit/4a6c8ef5952379d764f9773ec446c48c40f0ac51))
* **docs:** updated readme for developer build specifications ([7831aab](https://github.com/AniMathIO/AniMathIO/commit/7831aab1c7464d6dbdb84ed3bbbe9e051e189fb7))
* implemented custom modals for every supported components ([47b0ddd](https://github.com/AniMathIO/AniMathIO/commit/47b0ddd3752f7a0f6b83820f46557fce4006505c))
* implemented mathematical test elements ([0ba391c](https://github.com/AniMathIO/AniMathIO/commit/0ba391c5a68a4641c2ea47040ebacb26e4a11fa7))
* implemented modal for exporting [#21](https://github.com/AniMathIO/AniMathIO/issues/21) ([d9e8e00](https://github.com/AniMathIO/AniMathIO/commit/d9e8e0014b5c4f94f192e357cc2458af7d4cdf55))
* improvements ([7acec15](https://github.com/AniMathIO/AniMathIO/commit/7acec15eb5cd70f1484ec781ebbe44c146702e3d))
* increased build icon size ([8656445](https://github.com/AniMathIO/AniMathIO/commit/86564459c6845954fcf34cca2b3b5e7ae14499f6))
* initial commit ([3dedbdc](https://github.com/AniMathIO/AniMathIO/commit/3dedbdc1970cf6c983ccd69107e914cda21cf724))
* **mafs:** centered elements in the preview ([8c6e59f](https://github.com/AniMathIO/AniMathIO/commit/8c6e59f75d903c508507bdddb86db7701ecd5f8c))
* package.json update ([7eec4b2](https://github.com/AniMathIO/AniMathIO/commit/7eec4b2d384e0597f694cf888f7268bfd44cdbf9))
* package.json update ([f1429e2](https://github.com/AniMathIO/AniMathIO/commit/f1429e2bfaa4df9946adc1f35790266bc6a10e57))
* Update state.ts ([d0b78c8](https://github.com/AniMathIO/AniMathIO/commit/d0b78c83a2c80b2d6a0630891be0bc952aca8f3c))
* updated release please dependency ([6507157](https://github.com/AniMathIO/AniMathIO/commit/6507157186d7ce3f039b1a3280a74a4301d766ed))
* working on better media pool rendering and fixing mafs element render to canvas ([57e5304](https://github.com/AniMathIO/AniMathIO/commit/57e5304ba5191d10bdf053c5ce98cc8f9f08d44a))
* working on dynamical mafs element addition ([2ac2d24](https://github.com/AniMathIO/AniMathIO/commit/2ac2d24e3149ef7e44331465400f2be2a52f5c76))
* working on element rendering to canvas ([86899d0](https://github.com/AniMathIO/AniMathIO/commit/86899d0edda428ae13ee91d740473af829a29cb9))
* working on mafs element addition ([22e2a8d](https://github.com/AniMathIO/AniMathIO/commit/22e2a8d9d23db85acb7c35d5944eddd8cb4416ec))
* working on mafs integration ([c1e6e29](https://github.com/AniMathIO/AniMathIO/commit/c1e6e29de39fbf9f3c262a99970d97714e75d029))
* working on mafs integration ([da3dbaf](https://github.com/AniMathIO/AniMathIO/commit/da3dbaf96e6721b48b10c3800b8117bf541c2a84))
* working on manim element addition to canvas ([356d10d](https://github.com/AniMathIO/AniMathIO/commit/356d10dfd807c393b6b3e324e8200a29a800feab))
* working on windows build fixes ([0ee87c0](https://github.com/AniMathIO/AniMathIO/commit/0ee87c00e774e5eb7f00fa4cd9039961f9bf6e7e))


### Bug Fixes

* added dynamic loading for windows binary compilation ([11943b1](https://github.com/AniMathIO/AniMathIO/commit/11943b14a9d8c36277c3f52f2b58b7cbcf2463a3))
* **app:** fixed electron project loader, and added minimum window constraint ([4e46b6b](https://github.com/AniMathIO/AniMathIO/commit/4e46b6b880c95b0085cfaf3b47549890487e19db))
* **app:** fixed multiple rendering error but multiple audio doesnt works properly ([28ad912](https://github.com/AniMathIO/AniMathIO/commit/28ad912a44bf39308c18ab30c31ac4e092b38680))
* **app:** fixed rendering when no audio is set ([6630e8b](https://github.com/AniMathIO/AniMathIO/commit/6630e8b85a3aa727e8a0ac098711608209812220))
* **app:** removed typo ([168abba](https://github.com/AniMathIO/AniMathIO/commit/168abba803684ce1ce21a3d2523e6d62eedf2086))
* **app:** rendering errors ([7b7e9a0](https://github.com/AniMathIO/AniMathIO/commit/7b7e9a013897dcf0e528daae0abb8fc5ce4e6dcc))
* fixed aspect ratio ([db88d69](https://github.com/AniMathIO/AniMathIO/commit/db88d6913da25ac49c4b4a0564147f7c201daf3f))
* fixed error handling ([03bcb8d](https://github.com/AniMathIO/AniMathIO/commit/03bcb8d60bc819fb6197d6a4bac5d52dadbd9a01))
* fixed mafs element addition errors ([f35a8fe](https://github.com/AniMathIO/AniMathIO/commit/f35a8fec65b5839c94c422b08590aea22320cedd))
* fixed mathematical element names ([f161df0](https://github.com/AniMathIO/AniMathIO/commit/f161df07b1a01039293aab24e6354db7a1486d55))
* fixed release please yaml formatting ([68bc9da](https://github.com/AniMathIO/AniMathIO/commit/68bc9da9aa4fb1c17bed2dc60d63c617bc06155f))
* fixed type errors ([8f1aca0](https://github.com/AniMathIO/AniMathIO/commit/8f1aca076d4f40722e545519c4763c265c23fbbd))
* fixed windows build errors :) ([80e66e8](https://github.com/AniMathIO/AniMathIO/commit/80e66e859cf17fa0dfae91867dc384829aaf827b))
* maintainer name ([ae7e6ac](https://github.com/AniMathIO/AniMathIO/commit/ae7e6ac8113cfa5869e962b3381406cf8793caa1))
* resolved dependency problems ([19ad754](https://github.com/AniMathIO/AniMathIO/commit/19ad75498da43671d183b51177e82b2d2d2d3fef))
* **winapp:** added page extensions for windows app optimization ([9aba248](https://github.com/AniMathIO/AniMathIO/commit/9aba2480e4f9fe38f5f244bb3da169f7fcf61fad))
* working on fixing windows build, had to downgrade nextjs ([4f3e8c5](https://github.com/AniMathIO/AniMathIO/commit/4f3e8c5e2ddd47027ba4feb233cb855c94ef8451))
* working on modal for controlling mafs elements ([26c6ef3](https://github.com/AniMathIO/AniMathIO/commit/26c6ef32d85f08d29f97a035fe22641a1f741182))
* working on proper mafs element additions ([2adf124](https://github.com/AniMathIO/AniMathIO/commit/2adf12450929bd6736c38ab3ab76bf99d6224d3d))
* working on windows build errors ([0fced71](https://github.com/AniMathIO/AniMathIO/commit/0fced7122d11aa9d81c5d38a3323b0e30f78818e))


### Code Refactoring

* project structure refactoring for windows build ([7b0faa7](https://github.com/AniMathIO/AniMathIO/commit/7b0faa7efc2e1e6ef9c621d91b44267d6e147007))
