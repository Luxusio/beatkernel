# Retained WASAPI exclusive Source 교체 — 소프트웨어 검증 완료, native 실행 대기

기존 shared complete Source Owner를 같은 mode family의 exclusive 출력으로 확장한다.
원래 Mixer/PCM/history/command producer·Runtime·입력/capture와 attempt를 보존하며
실제 native Counter/frequency를 사용한다. 새 출력 상태 머신이나 fallback을 추가하지
않는다. [Source 계약](../runtime/REQ__retained-source-output-replacement.md)과
[WBS .13](../kernel/REQ__bms-player-work-breakdown.md)의 원래 전체 범위를 따른다.

Exclusive 이벤트 출력은 실제 native buffer 전체를 한 번에 acquire/release한다.
복구된 PCM이 새 buffer의 정수 배수이면 여러 lease로 그대로 제출하며 재생성하거나
패딩하지 않는다. Exact 설정에서 미리 아는 비배수는 native 호출 전에 거절하고,
default/명시적 rounding으로 최종 geometry가 미정이면 실제 GetBufferSize 뒤 history·
reconfigure·MMCSS·prefill 전에 확인한다. 비배수의 원본 complete owner는 보존한다.
[Microsoft GetBuffer](https://learn.microsoft.com/en-us/windows/desktop/api/audioclient/nf-audioclient-iaudiorenderclient-getbuffer)와
[GetCurrentPadding](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getcurrentpadding)의
exclusive event-buffer 조건을 따른다.

Portable worker와 Windows consumer를 분리된 source/test 쌍으로 작성하고 동결했다.
고정된 portable API의 실제 검사 뒤 native complete 연결의 source/test 쌍도 작성하고
동결했다. 아래 E70의 실제 독립 DEEP review/CLI 소프트웨어 QA는 PASS했으며,
새 desktop QA는 actual Windows 환경 부재로 BLOCKED_ENV다. 이전 shared 검사를
exclusive PASS로 확장하거나 이 결과를 실제 native 성공으로 해석하지 않는다.

## 실제 개발 검사

공통 production worker37 PASS/실패0(기존18·신규19), sizing22와 외부 worker
binary100 PASS다. 외부 binary는 Source 포함 중복 fixture와 실제 calibrated
allocation 검사1개를 포함하므로 다른 suite와 합산하지 않는다. 실제 System
allocator의 alloc/realloc/dealloc을 보정한 뒤 새 geometry/full lease·whole-multiple
복구·실패·conditional 처리에서 heap 호출0을 확인했다. 원본 PCM/report/prefix/history와
서로 다른 두 전체 lease의 실제 데이터, acquire/release/clock 오류를 검사했다.
사전 독립 coverage 점검에서 빠진 shared cold-helper 우회 분기를 보강했다.
실제 pending6/buffer4·EnginePeriod2조합/DeviceDefault1조합의 helper 호출 뒤
전체 owner 보존을 검사했다. 로그는 `portable-worker-final.log`와
`portable-allocator-final.log`이며 앞선36/99는 당시의 검사 기록이다.

Windows bin79 PASS/실패0에는 explicit blank device의 실제 회귀2개가 포함된다.
기존 parser가 명시적으로 비운 device를 이전 endpoint로 잘못 계승해 RED2를
재현했다. 공통 파서의 모순된 guard만 제거한 뒤 GREEN2와 전체79를 확인했다.
device 생략은 원래 endpoint 유지, 명시적 blank는 실제 OS default 조회, 명시적
nonempty는 exact endpoint다. actual default가 없으면 기존 오류를 보존한다.

로그는 ignored `target/wf/retained-source-exclusive-output-replacement/foundations/`의
`portable-worker-focused.log`, `portable-allocator-sizing.log`,
`explicit-default-device-actual-red.log`, `windows-bin-green.log`에 있다.
재현 명령은 platform `--lib wasapi_source_worker`, `--test audio_format
--test wasapi_source_worker_allocations`와 app `--bin windows_bms --no-default-features`
검사이며 기존 cached env/host CC/AR·locked·jobs2를 쓴다.

Windows platform/app의 lib/tests를 함께 C-stub Rust 형 검사해 exit0을 확인했다.
새 native fixture9개는 실제 Windows 실행 없이 typing만 확인했다. 실제 size guard는
GetBufferSize와 native buffer/period 일치 검사 뒤, history/reconfigure/MMCSS/prefill
전에 있다. Ready prefill과 running event는 같은 실제 mode selector를 사용한다.
이 순서의 실제 COM/resource/wake 실행은 native QA 범위다.

기존 실제 common pump4개를 포함한 Source Owner/UI37 PASS와 Source/Target
publication 외부 할당 각각1 PASS, 변경 Rust12개 scoped format/diff도 통과했다.
로그는 같은 foundations의 `windows-combined-types.log`,
`source-owner-pumps-regression.log`, `source-target-publication-allocations.log`에 있다.

platform/app의 경고 비차단 Clippy와 실제 `windows_bms --help`도 exit0이다.
경고가 남으므로 warning-clean/strict Clippy 판정은 아니다. 로그는
`platform-clippy.log`, `windows-bin-clippy.log`, `windows-public-help-smoke.log`다.
일반 health dry-run의 별도 진단값은0.0/10이었다. 위 명시적 환경에서 실제 실행한
scoped 검사나 제품 전체 검증의 대체물로 사용하지 않는다.

## 독립 최종 검증 — E70

동결 `bd9b8916`의 실제 정식 DEEP 리뷰는 FIX_NOW/INVESTIGATE/OPTIONAL0으로
PASS했다. 두 별도 discovery의 빈 결과를 최종 판정으로 대체하지 않고 실제18개
경로와 호출자·계약·기본 security/trust 경계를 정식 리뷰어가 다시 확인했다.
그 최종 응답 뒤 독립 CLI와 Windows desktop QA를 시작했다.

CLI 소프트웨어 QA는 app `desktop,webtransport` lib3258/0/ignored8,
bins624/0/ignored17, platform lib434/0/ignored2, worker37, 외부 worker100,
sizing22와 Source/Target publication 할당 각1을 통과했다. 외부100의 중복 fixture는
다른 suite와 합산하지 않는다. WASM browser-only lib, Windows/macOS app/platform
lib/tests C-stub typing, 경고 비차단 lib Clippy와 scoped Rust12 format/diff,
문서 링크144도 통과했다. Windows native 신규9개는 typing만 확인했고 미실행이다.

공개 명령26/26과 동시 render2프로세스를 실제 공백·한글 경로의 BMS/WAV로
검사했다. 결과 PCM은8000frames·32000bytes·비영샘플792·모든 값 finite였다.
기존 파일을 보존하며 동시 생성은 하나가 exit0, 다른 하나가 exit1로 거절됐다.
실제 제품 소스·공개 명령 검사이며 실제 Windows 재생이나 물리 성능 증거는 아니다.

새 desktop QA도 소유 Windows VM을 실제 부팅했다. IMMDeviceEnumerator 생성,
Render/AllStates 열거와 GetCount가 모두 HRESULT0이지만 endpoint는0개였다.
로그인 콘솔과 현재 player/rustc/cargo/cl도 없어 scoped native journey를 실행하지
못했다. 창 조회는 console-unavailable였고 스크린샷은 잠금 화면이다. VM은 강제
종료 없이 초기 stopped/Off·같은 incarnation으로 복구하고 후속 상태를 확인했다.
actual COM worker·clock/input·pause/replace/resume·PCM/capture·실패 정리는 남는다.

검증 로그는 `target/wf/retained-source-exclusive-output-replacement/` 아래
`qa-cli-bd9b8916/`의 `software-checks.json`, `public-cli-results.json`,
`final-evidence-summary.json`과 `qa-desktop-bd9b8916/`의 `availability.json`,
`restoration.json`, `windows-lock-screen.png`다. ignored 로그는 새 clone에 없을 수
있으므로 위 source/test와 계약·명시적 cached 환경의 명령으로 해당 scope를 재실행한다.
직접 final은 NON-ATTESTING이며 ordered hook-owned 증거 없이는 task close하지 않는다.
실제 결과가 모두 도착한 뒤 한 번의 `task_verify`는 PENDING을 반환했고 required
review/QA 완료 attestation이 누락돼 있다. 이것을 복구하거나 receipt만 얻기 위해
lens/verify를 재실행하지 않는다. 실제 desktop BLOCKED_ENV가 우선하는 미완료 조건이다.

WBS .13의 원래 문장을 유지하고4개 실행 단위로 나눠 활성220→223이다.
E68의105/223은 당시 기록이며 E70은 순수 worker 범위 .13.01만 V→D로 옮겨
D106·V72·W20·N2·E15·P5·U3, **106/223(47.53%)**다. native .13.02/.03은 V,
실제 장치 .13.04는 E이며 exclusive/Source 부모와 task/전체 Goal은 미완료다.
원래 제품 요구91/193(47.15%)과 다른 backend/형식/browser/물리/출시 요구를 유지한다.

재사용 가능한 규칙은 이미 커밋된 Source REQ와 실제 blank-device regression에
captured됐다: device 생략과 명시적 blank를 구분하고 actual OS default 부재를
정확히 거절한다. native 가용성·root 소유 cache 마찰은 확인된 환경 조건이다.
이번 작업에서 설치·권한 변경이나 새 검증 framework를 추가하지 않는다.

## Known ceiling

Known ceiling: 실제 exclusive native 동작은 아직 증명되지 않음 — 연결 구현과 독립 native QA에서 확인 필요

Known ceiling: 실제 exclusive 재생은 AC-004 native worker와 통합·Windows 검증 전까지 미검증 — 해당 구현과 검증이 완료되면 갱신

Known ceiling: 실제 Windows 장치 동작은 미검증 — 실행 가능한 native QA 환경에서 확인 필요

위 구현 레인의 연결과 형 검사는 E68에서 확인했고, 실제 native 실행 gate는 남는다.

직전 shared QA의 blocker를 새 E70에서도 실제 재확인했다. 위 native acceptance는
eligible Windows 환경에서 별도로 검증해야 한다. C-stub Rust typing은 native
실행이나 음향 증거가 아니다.
