# ASIO retained Source output — 진행 중

ASIO의 정확한 heterogeneous planar 출력 상태, 남은 원본 PCM과 제출 prefix,
original native interval을 기존 Source Owner·typed Output UI·solo/local pump에
연결하는 작업이다. 사전 독립 설계 검토 PASS 뒤 REQ/ADR을 먼저 기록했다.
아직 전체 구현·독립 소스 리뷰·QA 완료를 주장하지 않는다. 원래 BMS player
Goal과 WBS07.08.07.14의 실제 SDK/driver acceptance 요구는 유지한다.

기준 소스는 `a12ad712d4ff0be67e6c772a9bbfcdb24f7c5f54`다. 변경 전
Rust-only Windows GNU ASIO cfg check는 기존 `target/xcheck-asio-rust-only-12e0c`에
fingerprint를 쓰다가 I/O error5로 종료됐다. 이는 typing PASS도 소스 타입 오류
판정도 아니다. 실행 handle은 종료/소실했고 cargo/rustc가 남아 있지 않음을 확인했다.
후속 변경 검증에서 실제 결과를 별도로 기록한다. SDK feature build를 실행하거나
SDK/MSVC/C++ ABI를 입증한 것이 아니며 SDK/도구/driver를 설치하지 않는다.

## Known ceiling

Authentic MSVC/ASIO SDK build와 설치된 실제 driver/HWND/callback/음향 동작은
아직 미입증이다. Portable production seam 실행과 foreign Rust typing을 이
native acceptance로 확대하지 않는다. Callback retired와 성공한 native release는
별도 증거이며, 실패 시 소비된 handle의 stop 반복을 release 성공으로 취급하지 않는다.
Hook-owned 최종 완료 증거도 실제 리뷰·QA와 별도 gate다.
