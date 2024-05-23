# Changelog

## 1.0.0 (2024-05-23)


### Features

* add jwt secret for authentication ([#243](https://github.com/taikoxyz/raiko/issues/243)) ([78a633d](https://github.com/taikoxyz/raiko/commit/78a633da60c712c7338988d49031ff401a09d24f))
* add sgx input file lock to avoid concurrency problem ([9df1473](https://github.com/taikoxyz/raiko/commit/9df147394aa5e2c48a92364fcc037191faa914fd))
* Always respond with 200 but include status key ([#214](https://github.com/taikoxyz/raiko/issues/214)) ([9c13a4f](https://github.com/taikoxyz/raiko/commit/9c13a4fcb5e466ff0e190b52160c5d42b71f67e9))
* batch get 256 history headers ([#95](https://github.com/taikoxyz/raiko/issues/95)) ([fd3434a](https://github.com/taikoxyz/raiko/commit/fd3434aa72766e9cb0a74e20a2bfe784743ebbe2))
* let user setup which network to run ([#246](https://github.com/taikoxyz/raiko/issues/246)) ([9f80be5](https://github.com/taikoxyz/raiko/commit/9f80be559396dc1daccf3bce6f38b9b628d0a76e))
* lock the setup for sharing instance ([#244](https://github.com/taikoxyz/raiko/issues/244)) ([04d3197](https://github.com/taikoxyz/raiko/commit/04d31975f5417d3cb4357213174dbad3c178f81a))
* **raiko:** api versioning ([#196](https://github.com/taikoxyz/raiko/issues/196)) ([09e0005](https://github.com/taikoxyz/raiko/commit/09e0005d66d6e86d38381cab19c3990c1b0b7bae))
* **raiko:** Cherry-pick A7 updates([#182](https://github.com/taikoxyz/raiko/issues/182)) ([#197](https://github.com/taikoxyz/raiko/issues/197)) ([b3c2c1d](https://github.com/taikoxyz/raiko/commit/b3c2c1d9136348004f0a8653538cadf2743e8873))
* **raiko:** ci use sgx hw ([#175](https://github.com/taikoxyz/raiko/issues/175)) ([a40be21](https://github.com/taikoxyz/raiko/commit/a40be21d33d94414e4dc6259e17939785be69204))
* **raiko:** ci use sgx hw ([#175](https://github.com/taikoxyz/raiko/issues/175)) ([b7c44de](https://github.com/taikoxyz/raiko/commit/b7c44dedb784b39df9cbd7c87277f8355fa2fd50))
* **raiko:** enable kzg blob check ([#148](https://github.com/taikoxyz/raiko/issues/148)) ([9865b4c](https://github.com/taikoxyz/raiko/commit/9865b4cb91a56cbf0678d494cbea624f6ef0b067))
* **raiko:** generalized build pipeline for ZkVMs guests ([#133](https://github.com/taikoxyz/raiko/issues/133)) ([9cebd36](https://github.com/taikoxyz/raiko/commit/9cebd36a44c7243195b9cc1ef72ef2e949157dc1))
* **raiko:** install script + makefile CI integration ([#159](https://github.com/taikoxyz/raiko/issues/159)) ([a6c1095](https://github.com/taikoxyz/raiko/commit/a6c10953326b449127f6dcda2b92d2b1747c7f2d))
* **raiko:** load program from elf for risc zero ([#194](https://github.com/taikoxyz/raiko/issues/194)) ([dc0a427](https://github.com/taikoxyz/raiko/commit/dc0a4279cb8ad13cce54ce5ef182fe57509a6e3a))
* **raiko:** raiko object ([#149](https://github.com/taikoxyz/raiko/issues/149)) ([c4215bd](https://github.com/taikoxyz/raiko/commit/c4215bde45675d57e7a16f32107146b3b9756e75))
* **raiko:** read & merge chain spec from a optional config file ([#206](https://github.com/taikoxyz/raiko/issues/206)) ([4c76667](https://github.com/taikoxyz/raiko/commit/4c766678d8b0d1d9ba038e1f1be53679b25db05a))
* **raiko:** remove A6 support ([#200](https://github.com/taikoxyz/raiko/issues/200)) ([250b9ea](https://github.com/taikoxyz/raiko/commit/250b9ea21760442230573246a307c12816f42491))
* **raiko:** run general tests on all targets ([#164](https://github.com/taikoxyz/raiko/issues/164)) ([27b0bee](https://github.com/taikoxyz/raiko/commit/27b0beeaace5b93d1d32ac9b13da0722793fafeb))
* **raiko:** unity sgx & native proof response ([#223](https://github.com/taikoxyz/raiko/issues/223)) ([bead6e9](https://github.com/taikoxyz/raiko/commit/bead6e93542e264fae5c9faca7c726c8bd8d4ede))
* **raiko:** update chain spec ([#235](https://github.com/taikoxyz/raiko/issues/235)) ([8f21a69](https://github.com/taikoxyz/raiko/commit/8f21a690d82d3bc570bcc84f2ed4fa87a17ba6d7))
* **raiko:** update docker build ([#225](https://github.com/taikoxyz/raiko/issues/225)) ([e58c082](https://github.com/taikoxyz/raiko/commit/e58c082daf874ad57a60624ea92f29714a8f4c62))
* use batch api instead of customize api for history headers ([49d147f](https://github.com/taikoxyz/raiko/commit/49d147f54fc187a0cffd1767af47fcc5783496a6))
* use spec in setup ([#236](https://github.com/taikoxyz/raiko/issues/236)) ([cd097a5](https://github.com/taikoxyz/raiko/commit/cd097a5cca62ef8a7b8a40991939ff740e00dd22))
* use stdin instead of sgx tmp file ([0d33aa8](https://github.com/taikoxyz/raiko/commit/0d33aa81fadeab27e45e6632defa0e0d8ce293d4))


### Bug Fixes

* different manifest in docker and local ([#117](https://github.com/taikoxyz/raiko/issues/117)) ([52999d6](https://github.com/taikoxyz/raiko/commit/52999d664a44ad86f4a69392f76353fc656821ff))
* docker stuff ([#241](https://github.com/taikoxyz/raiko/issues/241)) ([6bb70b1](https://github.com/taikoxyz/raiko/commit/6bb70b15991060dba5620f7903a18008a02b43e3))
* enable the mpt cache ([#62](https://github.com/taikoxyz/raiko/issues/62)) ([46825d6](https://github.com/taikoxyz/raiko/commit/46825d66a2edfc8ce0e2acfb2e6e272645d79956))
* fetch history headers ([#100](https://github.com/taikoxyz/raiko/issues/100)) ([4fd70ee](https://github.com/taikoxyz/raiko/commit/4fd70eee7b5a64173549d3e466ab4bd7fbf2a33b))
* let config_path in config_dir ([#233](https://github.com/taikoxyz/raiko/issues/233)) ([78a5844](https://github.com/taikoxyz/raiko/commit/78a584406dde604b73b74e8269a7017cf6fb0098))
* **lib:** temporarily disable kzg check in sgx/sp1 provers ([#157](https://github.com/taikoxyz/raiko/issues/157)) ([039d2fa](https://github.com/taikoxyz/raiko/commit/039d2fae62a7ec7d66c40d73cc1a47c65bf87c23))
* metrics docker fix ([#216](https://github.com/taikoxyz/raiko/issues/216)) ([86bbc55](https://github.com/taikoxyz/raiko/commit/86bbc5598ee58194951a86c1775dfb30a3fed31b))
* mismatch method signature of libc's calloc ([#201](https://github.com/taikoxyz/raiko/issues/201)) ([ecde21d](https://github.com/taikoxyz/raiko/commit/ecde21da99ceeb273c3df736a152e9e6ab5ea23d))
* **raiko:** fix sticky invalid tx state ([#184](https://github.com/taikoxyz/raiko/issues/184)) ([99f5580](https://github.com/taikoxyz/raiko/commit/99f558088437af32e76e04d0529ea0715a163d40))
* **raiko:** make kzg work on SP1 ([#205](https://github.com/taikoxyz/raiko/issues/205)) ([027c3ae](https://github.com/taikoxyz/raiko/commit/027c3aee910a7a0cae1dec4eb19b7865d4aa5c0d))
* revm bn254 mul issue + cancun support + misc issues ([#222](https://github.com/taikoxyz/raiko/issues/222)) ([d90acd0](https://github.com/taikoxyz/raiko/commit/d90acd00be42b6af4a7f0301882d8719be5fdf64))


### Performance Improvements

* only filter once ([e1f5d41](https://github.com/taikoxyz/raiko/commit/e1f5d411a496a6d563ae8db61b164a0b77928884))
