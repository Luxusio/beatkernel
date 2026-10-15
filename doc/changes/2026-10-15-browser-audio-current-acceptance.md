# Current browser audio acceptance

현재 오디오 WASM을 예전 62,096바이트로 가정하던 검증을 실제 생성물의 크기와
외부 SHA256에 연결했다. `83330c7c`는 두 WASM identity를 출력/프로세스 생성 전에
확인하고 명시한20개 source snapshot을 고정해 실제 HTTPS 응답에 사용한다.
`53c18114`는 실제 page가 attach한 Worker CDP session을 같은 page를 통해 해제한다.
Production fetch/clock/audio/입력과13개 기존 사용자 여정은 변경하지 않았다.
오디오 무중단·무잡음 설정 변경 구현이나 전체 BMS player 완료를 뜻하지 않는다.

## Actual software evidence

기준은 `785f43c2563d4bd738d6339278a6b66af8feb30a`다. 두 차례 독립 DEEP 정식
code/security review는 각각 PASS0이며, 두 차례 hunter batch에서도 후보0이었다.
첫 정식 리뷰 뒤 `83330c7c`에서 전체 `app/web/*.test.mjs`를 한 번 실행해
**1516 PASS / 실패·취소·skip0**, exit0,223.899초를 확인했다. 현 생성 WASM의
오디오·준비·배속·메뉴·Worker binding 검사도 이 실행에 포함된다.
원본은 `/tmp/qa-cli-browser-audio-current-14041-1-node.log`다.

이후 변경은 driver cleanup 명령1곳과 문서뿐이다. 새 정식 리뷰 뒤 `53c18114`의
독립 CLI QA는 syntax/diff와 실제 preflight4종의 exit1·정확한 거절·출력 미생성을
재확인했다. 기존1516개를 재실행하거나 합산하지 않았다. 생성 JS/WASM4개와
production/unit target이 그대로임을 확인했다. 새 CLI 증거는
`target/wf/qa-cli-browser-audio-current-14041-2-l9d8ie0w/summary.json`이다.

기존 cached Rust1.98.1·wasm-bindgen0.2.129로 `browser-audio` release를 offline/
locked/jobs2 빌드한 직후 audio-pkg를 생성하고, `browser`를 빌드한 직후 pkg를
생성했다. 두 build/generator 모두 exit0이며 기존 unused/dead-code 경고가 있다.
명령·로그는 `/tmp/beatkernel-browser-current-build-14041.log`와
[재현 가이드](../verification/GUIDE__browser-worker-preparation-fixtures.md)를 따른다.

| WASM | Bytes | SHA256 |
| --- | ---: | --- |
| UI | 5,102,517 | `1a185ab2b7603ca2b43373c6302766b6640f8408d110d357e9972820e093b6d8` |
| Audio | 77,464 | `95d1483706a91eaa0c669ae98fe9a00f9231d9e9b02501b0361c714e1559bdcc` |

## Actual browser failure remains

독립 실행의 증거는 `target/wf/browser-audio-current-acceptance/qa-14041-1`과
`qa-14041-2`의 evidence.json·실제 PNG·exit1 로그에 남겼다. 둘 다 Settings/
Display/profile·trusted input·유한 live·capture·저장/목록/load/delete·나중 live
binding과 독립적인 replay까지13개 기존 여정을 완료했다. Live/replay는 hits1/
misses0이며, 각 실행의 실제 화면을 독립 QA가 열어 확인했다. Replay 종료 화면은
상태 문구와 레인 영역이며 live Results와 같은 결과 패널이라고 주장하지 않는다.

실제 server audio body는 외부 SHA와77,464bytes가 일치하고 Worker의 EOF1,
cancel0, release1, compile1, genuine Module/compileId/id가 대응한다. Window
오디오 준비와 대상 storage/body/construction은0, 종료 후 명시한 snapshot20개의
disk hash도 일치한다. UI 전체 transitive source closure를 고정한 증거는 아니다.

그런데 같은 실제 요청이 전체 데이터 직후 `net::ERR_ABORTED`/`canceled:true`를
Worker와 page 양쪽에서 보고하고 Worker loadingFinished는0이다. 두 실제 driver
실행은 strict assertion **FAIL(exit1)**을 유지한다. 본문 읽기·compile·여정 완료로
network error를 제외하지 않는다. 마지막 독립 browser lens는 이 실제 실패와
아래 엔진 환경 근거를 함께 확인해 **BLOCKED_ENV**로 판정했다.

첫 실행에는 cleanup의 `No session with given id`2건도 있었다. 실제 최소 Chromium
probe에서 root 연결의 child.detach가 실패하고 동일 child ID를 attaching page로
보내면 성공함을 변경 전에 확인했다. 수정 후2번째 실행은 cleanup error0,
browser/Xvfb exit0, 수집한 owned PID12개의 소멸과 전용 profile/cache/key/cert
삭제를 확인했다. 실제 HTTPS42763의 LISTEN 부재는 /proc/net/tcp와 tcp6로 확인했다.
이 환경의 connect_ex는 사용하지 않는 포트에도0을 반환하므로 종료 근거로 쓰지
않았다. 첫 실행의 실패 증거도 보존했다.

## Browser engine evidence

[Chromium change8508456](https://chromium-review.googlesource.com/c/chromium/src/+/8508456)은
no-store body를 getReader로 읽은 뒤 terminal state 기록 전 알림이 consumer cancel을
일으키는 같은 증상을 고친다. 2026-10-06 merge commit은
`62473ad0f747e245e514c93e741bb8e168a2a207`, main position1712735이며
[공식 issue568852319](https://issues.chromium.org/issues/568852319/dupes)와 연결된다.

현재 `/usr/bin/chromium`은 Chrome for Testing153.0.8010.12이고 실체는
`/opt/browsers/chromium-1243/chrome-linux64/chrome`이다.
[동일 버전 공식 directory metadata](https://chromium.googlesource.com/chromium/src/+/refs/tags/153.0.8010.12/third_party/blink/renderer/platform/loader/fetch/?format=JSON)의
response_body_loader.cc blob `cbb63f155cf8da2b9060adeca736e4cfbe00d8b8`가 patch의
old_sha와 같다. 독립 조사와 마지막 browser QA가 공식 메타데이터를 직접 확인했다.
관측과 일치하는 엔진 원인을 강하게 뒷받침하지만 수정된 엔진의 end-to-end PASS를
대신하지 않는다. 첫 fixed release/backport는 미확정이며, 현재 cache에서는 다른
수정 포함 browser를 찾지 못했다.

## Known ceiling

- Known ceiling: preflight negatives and real browser acceptance remain coordinator-owned — validate before marking AC-001 accepted.
- Known ceiling: 실제 브라우저 재검증은 coordinator 소유 — 기존 ERR_ABORTED 실패를 그대로 검증해야 함.

위 두 줄은 중간 구현 인계 때의 ceiling이다. 이후 preflight는 독립 QA로 확인했고
실제 browser도2회 검증했지만 strict acceptance는 미완료다. 수정된 loader를 포함한
브라우저 환경에서 같은 두 외부 해시·no-store·13개 전체 여정·엄격한 network/
cleanup 조건을 재확인해야 한다. 설치·다운로드, cache policy 변경이나 오류 제외로
성공을 대체하지 않는다. SwiftShader software 검증 범위이며 실제 GPU/IME/입력·
음향 지연이나 무중단 출력 전환은 미입증이다. 원래 Goal과 미완료 WBS는 유지한다.
