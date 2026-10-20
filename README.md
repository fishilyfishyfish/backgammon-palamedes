# Palamedes

백개먼 엔진(Rust → WASM)과 대국·분석 웹사이트. 설계와 진행 상황은 [CLAUDE.md](CLAUDE.md) 참고.

- 엔진 테스트: `cd engine && cargo test --release`
- 웹 빌드: `./scripts/build-wasm.sh` 후 `web/`을 정적 서버로 연다.
