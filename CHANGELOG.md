## [0.4.3](https://github.com/atuinsh/eye-declare/compare/v0.4.2..v0.4.3) - 2026-04-15

### 🐛 Bug Fixes

- Respect EventResult::Consumed in interactive loop ([#29](https://github.com/atuinsh/eye-declare/issues/29)) - ([512e881](https://github.com/atuinsh/eye-declare/commit/512e881a992a61ffa8823ed74fe16c14505840a5))
## [0.4.2](https://github.com/atuinsh/eye-declare/compare/v0.4.1..v0.4.2) - 2026-04-14

### 🐛 Bug Fixes

- Drain pending messages on exit so handle.fetch() succeeds ([#28](https://github.com/atuinsh/eye-declare/issues/28)) - ([83b69f6](https://github.com/atuinsh/eye-declare/commit/83b69f69044252053f9d164dc7db8ff74af6c97b))
## [0.4.1](https://github.com/atuinsh/eye-declare/compare/v0.4.0..v0.4.1) - 2026-04-14

### 🐛 Bug Fixes

- Re-render components when use_mount dirties state ([#27](https://github.com/atuinsh/eye-declare/issues/27)) - ([7d29d89](https://github.com/atuinsh/eye-declare/commit/7d29d899072ac2ffbfd3fb77548c97a6a3d3cadf))
## [0.4.0](https://github.com/atuinsh/eye-declare/compare/v0.3.0..v0.4.0) - 2026-04-09

### ⛰️ Features

- Add viewport component ([#24](https://github.com/atuinsh/eye-declare/issues/24)) - ([96dbf00](https://github.com/atuinsh/eye-declare/commit/96dbf00881a433a03d7088f18e3ce3d933b4ca76))
- Allow fetching app state ([#26](https://github.com/atuinsh/eye-declare/issues/26)) - ([190408a](https://github.com/atuinsh/eye-declare/commit/190408a149cd0e09963388a4ed8d9c17ad98f451))

### 🐛 Bug Fixes

- Persist interval effects across re-renders ([#25](https://github.com/atuinsh/eye-declare/issues/25)) - ([1d0841c](https://github.com/atuinsh/eye-declare/commit/1d0841ce385c238b725ed0146c1dd32d2036bfa9))
## [0.3.0](https://github.com/atuinsh/eye-declare/compare/v0.2.1..v0.3.0) - 2026-03-30

### ⛰️ Features

- [**breaking**] Hook callbacks receive props — Hooks<S> becomes Hooks<P, S> ([#20](https://github.com/atuinsh/eye-declare/issues/20)) - ([8b67f86](https://github.com/atuinsh/eye-declare/commit/8b67f86082932016721ad632f01b62a36b86fda5))
- [**breaking**] Replace TextBlock/Line with Text component ([#19](https://github.com/atuinsh/eye-declare/issues/19)) - ([01790c8](https://github.com/atuinsh/eye-declare/commit/01790c837700c2b3c7e4f906170009b3198058c0))
- Wave 4 — use_height_hint, data children in #[component], HooksOutput cleanup ([#18](https://github.com/atuinsh/eye-declare/issues/18)) - ([d760eb6](https://github.com/atuinsh/eye-declare/commit/d760eb6d5e35916a4777533b63166238e64ffffc))
- Wave 3 — structural simplification (single-call update, hidden legacy methods) ([#17](https://github.com/atuinsh/eye-declare/issues/17)) - ([075a46e](https://github.com/atuinsh/eye-declare/commit/075a46e62a791d127eda8c77aceb00f0de2ff506))
- Wave 2 — migrate built-in components to #[component] fn model ([#16](https://github.com/atuinsh/eye-declare/issues/16)) - ([6bbcef8](https://github.com/atuinsh/eye-declare/commit/6bbcef87bba532320fe16a2be3f83ad65fe55a7a))
- Wave 1 — internals cleanup for #[component] fn transition ([#15](https://github.com/atuinsh/eye-declare/issues/15)) - ([80f287e](https://github.com/atuinsh/eye-declare/commit/80f287e4e65613abcbf4a4f6b5db1dfde97223f0))
- [**breaking**] Function components with #[props], #[component], and behavioral hooks ([#14](https://github.com/atuinsh/eye-declare/issues/14)) - ([f3e681d](https://github.com/atuinsh/eye-declare/commit/f3e681dc4ed968ea89125bad8bd6bf2bf7050b13))
- [**breaking**] Make view() primary, remove children()/uses_view(), add DataChildren<T> ([#13](https://github.com/atuinsh/eye-declare/issues/13)) - ([d9680ff](https://github.com/atuinsh/eye-declare/commit/d9680ff9d3a8b45f7eb0ec17a3d7db060ea3f77f))
- Unified view() method + Canvas component ([#12](https://github.com/atuinsh/eye-declare/issues/12)) - ([7220802](https://github.com/atuinsh/eye-declare/commit/72208027a7732e885b4eb7e36d5f5c83cf08ba87))
- Eliminate required desired_height via probe render measurement ([#10](https://github.com/atuinsh/eye-declare/issues/10)) - ([5ba8072](https://github.com/atuinsh/eye-declare/commit/5ba80726c097a086e0510b500db59448aff8b1c5))
- Add View component for unified layout + chrome ([#2](https://github.com/atuinsh/eye-declare/issues/2)) - ([96dc51c](https://github.com/atuinsh/eye-declare/commit/96dc51cd32a22fe340feafb801e6ca32ec309f7e))

### 🐛 Bug Fixes

- #[props] uses re-exported TypedBuilder path - ([e59ac94](https://github.com/atuinsh/eye-declare/commit/e59ac94c40002bd17fc8aa22b1afcc6703a49041))

### 📚 Documentation

- Fix stale typed_builder:: reference in props doc comment - ([96b44bc](https://github.com/atuinsh/eye-declare/commit/96b44bc05224e4cbe774828ed29714638bfa217e))

### ⚡️ Performance

- Eliminate probe rendering for Text and Markdown ([#21](https://github.com/atuinsh/eye-declare/issues/21)) - ([1d02015](https://github.com/atuinsh/eye-declare/commit/1d020153b2931e30f3ff3060b3459ae1afb6662b))
## [0.2.1](https://github.com/atuinsh/eye-declare/compare/v0.2.0..v0.2.1) - 2026-03-28

### ⛰️ Features

- Add focus scopes for scoped Tab cycling ([#1](https://github.com/atuinsh/eye-declare/issues/1)) - ([bb4da07](https://github.com/atuinsh/eye-declare/commit/bb4da076148d20fa59bcecf8bc93216cb7a5f9ae))

### 📚 Documentation

- Mention DEC 2026 synchronized output in key features - ([305947b](https://github.com/atuinsh/eye-declare/commit/305947b61f5a0f3ff47c6ead1d15e58b836310ad))
## [0.1.1] - 2026-03-25
<!-- generated by git-cliff -->
