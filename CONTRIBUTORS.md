# Contributors

Thank you to everyone who has contributed code, ideas, or feedback to
typst-as-lib!

## Contributors

- [Relacibo](https://github.com/Relacibo) — creator and maintainer
- [hungrybluedev](https://github.com/hungrybluedev) — CI/CD workflow and lint fixes ([#20](https://github.com/Relacibo/typst-as-lib/pull/20))
- [niusia-ua](https://github.com/niusia-ua) — more informative Typst errors ([#13](https://github.com/Relacibo/typst-as-lib/pull/13))
- [ta-vroom](https://github.com/ta-vroom) — builder API fix for `comemo_evict_max_age` ([#58](https://github.com/Relacibo/typst-as-lib/pull/58))
- [malted](https://github.com/malted) — documentation fix for the `packages` feature ([#17](https://github.com/Relacibo/typst-as-lib/pull/17))

Thanks to [dependabot](https://github.com/dependabot) for keeping the
dependencies fresh.

## Special thanks

- [lukas-loering](https://github.com/lukas-loering) — designed the approach for
  printable compilation diagnostics
  ([#53](https://github.com/Relacibo/typst-as-lib/pull/53)). The PR was not
  merged as-is, but its core idea shaped
  [`diagnostic::SourceDiagnostics`](https://docs.rs/typst-as-lib/latest/typst_as_lib/diagnostic/index.html)
  (0.18.0), where he is also credited.

## AI assistance

typst-as-lib itself was built by Relacibo without AI. Some features and fixes
on top of it were developed with AI assistance — e.g. `with_world` /
`world_builder` (0.15.5), the `time` module (0.17.1) and
`diagnostic::SourceDiagnostics` (0.18.0), including most doc
comments. Design and review happen with Opus 5.5, implementation with GLM 5.3
Flash (OpenCode); Relacibo reviews every change before it lands.
