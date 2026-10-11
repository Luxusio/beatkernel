# 고정 실시간 플레이 배속 — 구현 진행 중

실시간 플레이가 원곡 좌표를 유지하면서 양의 유리수 배속을 선택하도록
공통 Transport와 출력 스케줄링을 확장하고 있다. 입력 수집 시계와 실제
키음 출력 시각은 배속을 곱하지 않는다. 출력 preroll과 일시정지 시간은
출력 시간 단위로 유지하며, 원곡 위치는
`start + trunc((active_elapsed - preroll) * numerator / denominator)`로 계산한다.

## 현재 개발 검증

- 새 Transport 계약 테스트 16개와 기존 시간·Transport 테스트 10개가 통과했다.
  홀수 나노초 preroll, 실제 Runtime 판정·입력 provenance·키음 출력 시각,
  일시정지 후 분수 위상, 역사적 조회, 원자적 거절과 정수 범위를 검사한다.
  원본 로그: `target/wf/live-speed-development-12d03/transport-2.log`.
- 브라우저 공통 모델의 새 테스트 14개와 기존 테스트 44개가 통과했다.
  기존 replay 모델 함수를 직접 export alias로 재사용한다.
  `node-model-result.json`은 coordinator의 실행 요약이며 원본 TAP이나
  독립 QA 증거를 대신하지 않는다.
- 공통 `LiveSpeedSetup` 계약 테스트 14개가 통과했다. 네 배속의 원곡 위치와
  BGM 역변환, 음수 시작점, 출력 도메인 보존, 0 프레임 종료점, 실제 frame grid
  올림과 전체 signed span의 마지막 범위 검사를 확인했다.
  원본 로그: `target/wf/live-speed-development-12d03/live-setup-1.log`.
- native cold 오디오 계약 테스트 11개가 통과했다. 실제 Mixer PCM, 배경음이
  없는 경우의 origin SetRate, 원시 입력·Stop 시각, finite endpoint와 큐 credit,
  시작 gate 및 NORMAL 호환성을 검사했다.
  원본 로그: `target/wf/live-speed-development-12d03/native-audio-1.log`.
- 새 pause 계약 14개, 새 Settings/profile 계약 14개와 기존 app 단위 테스트
  1960개가 통과했다. 같은 실행에서 공통 설정 14개와 오디오 11개도 다시
  통과했다. 기존 app의 ignored 4개는 실행하지 않았으며, 직접 장치 검증을
  대신하지 않는다. 원본 로그:
  `target/wf/live-speed-development-12d03/shared-regressions-1.log`.
  source/converted target/interval pause의 실제 acknowledgement, 분수 위상,
  원자적 거절, 실제 AudioAuthority token, 기존 Settings 행 순서와 profile
  v1/v2 기본 바이트 호환성을 확인했다.
- 기록 배속 계약 테스트 13개가 통과했다. NORMAL 바이트 보존, 잘못된
  envelope 거절과 전체 header 예산, 실제 Runtime 입력·판정·gauge·hash·
  provenance의 기록/복원, 별도 replay 출력 배속 및 native catalog 조건
  비교를 검사했다. 원본 로그: `live-recording-4.log` (같은 증거 폴더).
  앞선 실행의 test fixture 정수 타입, WAV 정의와 두 탭 사이 release 오류는
  각각 고쳤으며 `live-recording-1.log`부터 `live-recording-3.log`에 보존했다.
  파서나 판정 검증을 완화하지 않았다. source-speed 추가 이후의 전체 app
  회귀와 실제 플레이 소비자·독립 QA는 아직 남아 있다.
- 공통 solo/local 플레이의 새 테스트 14개와 native policy의 public 호환
  테스트 1개가 `live-owners-3.log`에서 통과했다. 실제 네 배속의 Mixer PCM,
  키음·mine·gauge, 원곡 입력/기록 복원, activation, zero/quantized finite end,
  실제 출력 표식·presentation·credit, local 2/4/64 member 종료를 검사한다.
  64 member는 순수 component fixture이며 64 물리 장치 검증이 아니다.
  유한 종료에서 실제 SetRate credit을 회수하지 못하던 결함은 검증된
  RenderReport cursor로 반납하도록 수정했다. 최초 실패는 `step-live-1.log`에
  보존했다. 새 native private admission 12개를 포함한 app 단위 테스트
  1972개와 확장된 기록 테스트 15개는 `live-owners-2.log`에서 통과했다.
  해당 실행의 public fixture WAV 정의 오류는 이후 수정했고, Step의 마지막
  credit 수정 이후 전체 app 단위 테스트 재실행은 최종 회귀 단계에 남아 있다.
- 최초 전체 core 검사 `core-full-1.log`는 컴파일 중 240초 제한으로 종료됐다
  (exit 124). 제한을 600초로 늘린 후 같은 source를 재검사했고
  `core-full-2.log`에서 62개 suite의 531개 테스트가 모두 통과했다
  (exit 0, failed 0, ignored 0, doctest 포함).
- 실제 native/browser 소비자 연결과 독립
  review·QA는 아직 진행 중이다. 새 cold API의 통과는 아직 실제 앱에서
  배속을 선택하고 플레이하는 전체 경로의 완료를 의미하지 않는다.

## 남은 범위

이 문서는 개발 중간 증거다. 현재 태스크의 독립 code/security review와
CLI/browser/desktop QA가 완료됐다는 주장이 아니다. 고정 실시간 배속의
전체 연결, 실행 중 배속 변경, retained practice/seek/loop 배속, 역재생과
물리 장치 검증은 각각 남은 범위에 따라 계속 구현·검증한다.
WBS는 91/193 (47.15%)를 유지하며 전체 BMS player Goal은 미완료다.
