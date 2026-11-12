# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## [v0.32.2](https://github.com/Akurganow/sound-to-patch/compare/03982896c2e899992a099284a1fe1f6c1c403459..v0.32.2) - 2026-11-12
#### Bug Fixes
- (**units**) read every unit string and millisecond value the same way (#345) - ([4693721](https://github.com/Akurganow/sound-to-patch/commit/469372121b18bbfa65fbf4f41d02d7effdf0ca1e)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.32.1](https://github.com/Akurganow/sound-to-patch/compare/4c008691bb9b9525de1fe949a2d7454c39461126..v0.32.1) - 2026-11-12
#### Bug Fixes
- ship a man page for every subcommand (#342) - ([f7890ec](https://github.com/Akurganow/sound-to-patch/commit/f7890ec6a8c38955fdc5c6512e4fb8ef922038df)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.32.0](https://github.com/Akurganow/sound-to-patch/compare/35065944ef5fca7d9589d61f58e81c1446aa0470..v0.32.0) - 2026-11-12
#### Features
- (**analyze**) keep the old report when a write fails and tell stems apart by checksum (#336) - ([3506594](https://github.com/Akurganow/sound-to-patch/commit/35065944ef5fca7d9589d61f58e81c1446aa0470)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5
#### Bug Fixes
- (**analyze**) bound the sample rate and keep the intake conventions true at every rate (#338) - ([45710a5](https://github.com/Akurganow/sound-to-patch/commit/45710a59f7efcdacd31c89f27368b027a7bc867f)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.31.2](https://github.com/Akurganow/sound-to-patch/compare/ca2d8fcf54e7b418c37afc89fd7b72614a9f3e73..v0.31.2) - 2026-11-12
#### Bug Fixes
- (**render**) repair the spread tool and name the real cause of a render failure (#339) - ([ca2d8fc](https://github.com/Akurganow/sound-to-patch/commit/ca2d8fcf54e7b418c37afc89fd7b72614a9f3e73)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.31.1](https://github.com/Akurganow/sound-to-patch/compare/5fa0c801bce8addc67b0674ceea035c2c7bce873..v0.31.1) - 2026-11-12
#### Bug Fixes
- (**preset**) refuse edits and presets with the right cause, and never damage a preset (#337) - ([5fa0c80](https://github.com/Akurganow/sound-to-patch/commit/5fa0c801bce8addc67b0674ceea035c2c7bce873)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.31.0](https://github.com/Akurganow/sound-to-patch/compare/6351c3dc31b8ec17dd17d2dc69122b53ba6f9d22..v0.31.0) - 2026-11-05
#### Features
- (**notes**) write the final notes.mid from the report (#324) - ([6351c3d](https://github.com/Akurganow/sound-to-patch/commit/6351c3dc31b8ec17dd17d2dc69122b53ba6f9d22)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.30.1](https://github.com/Akurganow/sound-to-patch/compare/4cf3326a6468f2ccb84abb6f50cc136d7e3f4f5f..v0.30.1) - 2026-11-05
#### Bug Fixes
- (**analyze**) read whether a stripped note has ended on the unstripped level (#325) - ([4cf3326](https://github.com/Akurganow/sound-to-patch/commit/4cf3326a6468f2ccb84abb6f50cc136d7e3f4f5f)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.30.0](https://github.com/Akurganow/sound-to-patch/compare/8c6e9a09d45a84280a043c15049ff4e6ce73c380..v0.30.0) - 2026-11-05
#### Features
- (**preset**) plan pitch and envelope edits for every synth (#321) - ([8c6e9a0](https://github.com/Akurganow/sound-to-patch/commit/8c6e9a09d45a84280a043c15049ff4e6ce73c380)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.29.0](https://github.com/Akurganow/sound-to-patch/compare/1013e2f114b7e00abde6d4283b62360b7b6eb152..v0.29.0) - 2026-11-05
#### Features
- (**skill**) write the Vital control dictionary in the layer files (#318) - ([7957c8d](https://github.com/Akurganow/sound-to-patch/commit/7957c8d3a135d2060b6e8d7cee87efbc16571963)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.28.1](https://github.com/Akurganow/sound-to-patch/compare/e3c0fe725f199c5cd7a2d50360b6a9a87e161e80..v0.28.1) - 2026-11-04
#### Bug Fixes
- (**analyze**) keep the note-off knee from the next note's corner after a long gap (#315) - ([91cf23e](https://github.com/Akurganow/sound-to-patch/commit/91cf23e8b7ba29819f8eea8e6639628a2123d521)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.28.0](https://github.com/Akurganow/sound-to-patch/compare/464bb181266617ab745abb10014dbb967b642fbf..v0.28.0) - 2026-11-01
#### Features
- (**analyze**) fit the release and tail after the note-off, and vote on reverb - ([804daf4](https://github.com/Akurganow/sound-to-patch/commit/804daf4b00ca243a89eebf3a0a08326a4b1a45e4)) - [@Akurganow](https://github.com/Akurganow)

- - -

## [v0.27.0](https://github.com/Akurganow/sound-to-patch/compare/a930c52258b56f6fb9e5c9b2d5b205579d50e95a..v0.27.0) - 2026-10-31
#### Features
- (**analyze**) count voices and read how a mono line joins its notes (#309) - ([a930c52](https://github.com/Akurganow/sound-to-patch/commit/a930c52258b56f6fb9e5c9b2d5b205579d50e95a)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.26.0](https://github.com/Akurganow/sound-to-patch/compare/a95a7b23ed93bbefe0a4381e1ae29ec20cd93fd0..v0.26.0) - 2026-10-31
#### Features
- (**analyze**) report the amplitude envelope and the sidechain pump (#307) - ([a713c34](https://github.com/Akurganow/sound-to-patch/commit/a713c34eda38b1031f2162e1609c05c32ac5b87b)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.25.0](https://github.com/Akurganow/sound-to-patch/compare/e2307d36076451e017c450cb217c63017f186e33..v0.25.0) - 2026-10-30
#### Features
- (**notes**) split decoded notes at attacks inside them (#305) - ([e2307d3](https://github.com/Akurganow/sound-to-patch/commit/e2307d36076451e017c450cb217c63017f186e33)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.24.0](https://github.com/Akurganow/sound-to-patch/compare/13f93860d065726b07b7ff265a1e407ca544a8e0..v0.24.0) - 2026-10-30
#### Features
- (**analyze**) report glides, pitch drops and vibrato (#287) - ([13f9386](https://github.com/Akurganow/sound-to-patch/commit/13f93860d065726b07b7ff265a1e407ca544a8e0)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.23.1](https://github.com/Akurganow/sound-to-patch/compare/31f81d953f46e4a1cfc6824fdeb389aea7a0941a..v0.23.1) - 2026-10-29
#### Bug Fixes
- (**notes**) keep repeated gated notes apart in the split join (#279) - ([f9db972](https://github.com/Akurganow/sound-to-patch/commit/f9db97269029abcf673c69b3ca13a70afe0f71dc)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.23.0](https://github.com/Akurganow/sound-to-patch/compare/aa8c2306e603fcf590b8501fd8ec035d8b4ee486..v0.23.0) - 2026-10-29
#### Features
- (**render**) render single notes in the open Vital engine (#298) - ([aa8c230](https://github.com/Akurganow/sound-to-patch/commit/aa8c2306e603fcf590b8501fd8ec035d8b4ee486)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.22.0](https://github.com/Akurganow/sound-to-patch/compare/9eed99f8bce40dff8ddae02be042f96db340ff9d..v0.22.0) - 2026-10-29
#### Features
- (**render**) measure the repeat-render spread and compare reference recordings (#297) - ([9eed99f](https://github.com/Akurganow/sound-to-patch/commit/9eed99f8bce40dff8ddae02be042f96db340ff9d)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.21.1](https://github.com/Akurganow/sound-to-patch/compare/a3a2cc2a5413a31c671ebd39707a00fcdd0c1853..v0.21.1) - 2026-10-29
#### Bug Fixes
- (**deps**) write the pYIN f0 tracker in the repo and drop zip and atty (#294) - ([9079bbb](https://github.com/Akurganow/sound-to-patch/commit/9079bbb2453c19425f3185977ae64f7d1c356a7b)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.21.0](https://github.com/Akurganow/sound-to-patch/compare/95440bf6104ac8d7de391daa56d71d64026a5249..v0.21.0) - 2026-10-29
#### Features
- (**render**) run renders in a render session and time them (#295) - ([95440bf](https://github.com/Akurganow/sound-to-patch/commit/95440bf6104ac8d7de391daa56d71d64026a5249)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.20.1](https://github.com/Akurganow/sound-to-patch/compare/9f723672a040b7a8bf0be3ea54a6647fb4638069..v0.20.1) - 2026-10-29
#### Bug Fixes
- (**render**) check the saved tempo against the render tempo (#296) - ([9f72367](https://github.com/Akurganow/sound-to-patch/commit/9f723672a040b7a8bf0be3ea54a6647fb4638069)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.20.0](https://github.com/Akurganow/sound-to-patch/compare/f6870991a3a8e29d2876702fda2ca964748a038d..v0.20.0) - 2026-10-29
#### Features
- (**skill**) add the word dictionary (#289) - ([f687099](https://github.com/Akurganow/sound-to-patch/commit/f6870991a3a8e29d2876702fda2ca964748a038d)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.19.0](https://github.com/Akurganow/sound-to-patch/compare/334d9aa6654d1fe705801c0432c261c6d163d7fb..v0.19.0) - 2026-10-29
#### Features
- (**render**) add the render command with the load check (#284) - ([ee51a83](https://github.com/Akurganow/sound-to-patch/commit/ee51a83b5cdc38a7d44aaf6c7bf6ced69ae5b198)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.18.0](https://github.com/Akurganow/sound-to-patch/compare/c411d3fc51781df026908695301287298b4d19cb..v0.18.0) - 2026-10-28
#### Features
- (**render**) render Vital presets through a plug-in host process (#283) - ([084fd4f](https://github.com/Akurganow/sound-to-patch/commit/084fd4fe382617bf6835cdf89c0bf5b994644b56)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.17.0](https://github.com/Akurganow/sound-to-patch/compare/62d498bd729e9cfeabddd9176e04ca2e342243b9..v0.17.0) - 2026-10-28
#### Features
- (**skill**) add the music dictionary (#282) - ([62d498b](https://github.com/Akurganow/sound-to-patch/commit/62d498bd729e9cfeabddd9176e04ca2e342243b9)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.16.0](https://github.com/Akurganow/sound-to-patch/compare/0ec48fe7b67fbd1433e571ed8ff8d969e5bb709b..v0.16.0) - 2026-10-28
#### Features
- (**analyze**) refine the tempo and run the notes step inside analyze (#277) - ([0ec48fe](https://github.com/Akurganow/sound-to-patch/commit/0ec48fe7b67fbd1433e571ed8ff8d969e5bb709b)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.15.0](https://github.com/Akurganow/sound-to-patch/compare/76169d0195d6d1b2096451cecddb80881b25aef7..v0.15.0) - 2026-10-28
#### Features
- (**render**) add the render recipe, the renderer trait and a fake renderer (#275) - ([76169d0](https://github.com/Akurganow/sound-to-patch/commit/76169d0195d6d1b2096451cecddb80881b25aef7)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.14.0](https://github.com/Akurganow/sound-to-patch/compare/531f63555ac0c2cd4a195d89e4eedafd5794519e..v0.14.0) - 2026-10-28
#### Features
- (**skill**) add the shared agent reference with its plugin, catalogue and checks (#276) - ([531f635](https://github.com/Akurganow/sound-to-patch/commit/531f63555ac0c2cd4a195d89e4eedafd5794519e)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.13.0](https://github.com/Akurganow/sound-to-patch/compare/3d3fdccb51727878d3da0c4dc33153fd3a238cd4..v0.13.0) - 2026-10-28
#### Features
- (**notes**) add the notes command, clean up decoded notes and bench the step (#242) - ([3d3fdcc](https://github.com/Akurganow/sound-to-patch/commit/3d3fdccb51727878d3da0c4dc33153fd3a238cd4)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.12.0](https://github.com/Akurganow/sound-to-patch/compare/cb3b871a47325baf0868db080a6bd4a7104397b2..v0.12.0) - 2026-10-27
#### Features
- (**notes**) track f0, detect the register and voicing, and evaluate the notes step (#241) - ([cb3b871](https://github.com/Akurganow/sound-to-patch/commit/cb3b871a47325baf0868db080a6bd4a7104397b2)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.11.0](https://github.com/Akurganow/sound-to-patch/compare/2fa0259b7f60a51587a45f3bda8234b7be6c041f..v0.11.0) - 2026-10-27
#### Features
- (**notes**) transcribe a stem and write a MIDI file from a note list (#239) - ([91f2765](https://github.com/Akurganow/sound-to-patch/commit/91f2765f407293e1ee69606d1a18550a52e7b9ad)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.10.0](https://github.com/Akurganow/sound-to-patch/compare/2652bceb01fa290e25dbd76ca80dafbb3473ba6c..v0.10.0) - 2026-10-27
#### Features
- (**analyze**) add the analyze command with layers.json, its schema and the spectrogram (#236) - ([2652bce](https://github.com/Akurganow/sound-to-patch/commit/2652bceb01fa290e25dbd76ca80dafbb3473ba6c)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.9.0](https://github.com/Akurganow/sound-to-patch/compare/88696c5510bce3995f64a94a832b4d04c60f8705..v0.9.0) - 2026-10-27
#### Features
- (**preset**) write any Vital effect order and LFO shapes as outputs (#237) - ([88696c5](https://github.com/Akurganow/sound-to-patch/commit/88696c5510bce3995f64a94a832b4d04c60f8705)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.8.0](https://github.com/Akurganow/sound-to-patch/compare/5735b1a97848f7d8a188e43b857168d52a9868d7..v0.8.0) - 2026-10-27
#### Features
- (**analyze**) add the stem, stereo screen, descriptors and modulation estimators (#235) - ([5735b1a](https://github.com/Akurganow/sound-to-patch/commit/5735b1a97848f7d8a188e43b857168d52a9868d7)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.7.0](https://github.com/Akurganow/sound-to-patch/compare/0f243de0b3afbc83f0628f1233003a10ff9a5f20..v0.7.0) - 2026-10-27
#### Features
- (**preset**) verify Vital presets on the open engine and 1.0.7, and document them (#234) - ([0f243de](https://github.com/Akurganow/sound-to-patch/commit/0f243de0b3afbc83f0628f1233003a10ff9a5f20)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.6.0](https://github.com/Akurganow/sound-to-patch/compare/78147273d99bb76d62c17c466dcb3284be1cb6c2..v0.6.0) - 2026-10-26
#### Features
- (**preset**) write Vital presets with preset apply (#232) - ([7814727](https://github.com/Akurganow/sound-to-patch/commit/78147273d99bb76d62c17c466dcb3284be1cb6c2)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.5.0](https://github.com/Akurganow/sound-to-patch/compare/a7cf4cc5ba0af89a26ab99f155e74c251d72a85f..v0.5.0) - 2026-10-26
#### Features
- (**analyze**) add stem intake, note masks and the signal core - ([a7cf4cc](https://github.com/Akurganow/sound-to-patch/commit/a7cf4cc5ba0af89a26ab99f155e74c251d72a85f)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.4.0](https://github.com/Akurganow/sound-to-patch/compare/6da52b8fbec72f603cc5e69c828921ba24d46ef9..v0.4.0) - 2026-10-26
#### Features
- (**preset**) add the synth seam and read Vital presets - ([f394b21](https://github.com/Akurganow/sound-to-patch/commit/f394b21b8b86a92bcb34f36efc2e7a1532d2b0b4)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.3.0](https://github.com/Akurganow/sound-to-patch/compare/d3ebab3766153bf2e1349507ee01f7ddc10d8c78..v0.3.0) - 2026-10-26
#### Features
- rename the program to sound-to-patch - ([d3ebab3](https://github.com/Akurganow/sound-to-patch/commit/d3ebab3766153bf2e1349507ee01f7ddc10d8c78)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.2.0](https://github.com/Akurganow/sound-to-serum/compare/215579fe007e78a91841cc23bf7390ea8eefce5f..v0.2.0) - 2026-10-26
#### Features
- (**units**) add unit types, conversions, unit strings and Confidence (#165) - ([cadea5d](https://github.com/Akurganow/sound-to-serum/commit/cadea5d4111dd3e5e3bcd5a62f51fde3dc980b43)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5

- - -

## [v0.1.0](https://github.com/Akurganow/sound-to-serum/compare/d7a8b95862af9c074a0f76b263297f391a52406e..v0.1.0) - 2026-10-25
#### Features
- add the sound-to-serum command with its help and version (#160) - ([a20efc7](https://github.com/Akurganow/sound-to-serum/commit/a20efc759807e8174f75ab0860a98931f05956d6)) - [@Akurganow](https://github.com/Akurganow), Claude Opus 5.5
#### Bug Fixes
- (**ci**) reject workflow skip markers in the pull request description (#162) - ([020f4b9](https://github.com/Akurganow/sound-to-serum/commit/020f4b9940ca16884f6c30321626d6e8162a157d)) - [@Akurganow](https://github.com/Akurganow)

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).