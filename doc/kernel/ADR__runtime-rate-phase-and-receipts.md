# 실행 중 배속의 정확한 phase와 적용 결과

상태: 채택, 구현 중. 동작 계약은 [runtime 배속](REQ__runtime-playback-rate.md)을
따른다. 아직 전체 경로가 구현·검증됐다는 뜻이 아니다.

## 문제와 결정

정수 곡 위치로 전환 anchor를 다시 만들면 기존 분수 ns가 버려진다. 각 프레임의
경과 시간을 따로 반올림해 더하는 방법도 같은 원본 sample clock과 달라진다.
고정 배속을 실행 중 SetRate 하나로 바꾸면 이미 예약한 BGM과 종료 시점이 남는다.

기존 active output elapsed E에 대해 signed rational Q를 보관하는 작은 Copy
PositiveRatePhase를 추가하고 Practice Cursor와 exact Transport가 공유한다.
새로운 물리 clock이나 보정기를 만들지 않는다. S+trunc(Q)의 S를 따로 유지하며
원래 preroll부터 각 실제 rate cut까지 분수 전체를 보존한다.

floor 정수 i128과 reduced proper u64 분수를 사용한다. 일반적인 delta 계산은
분모로 먼저 나누고, fraction 덧셈은 u128에서 약분한 뒤 좁힌다. 미래 정수 E의
공통 분모가 저장 범위를 넘으면 해당 sequence를 capacity 오류로 거절한다.
전역 Rate 범위를 축소하거나 callback에 큰 정수 heap 연산을 넣지 않는다.

역산은 (target-whole-1)을 양수 rate 분자로 먼저 나눠 큰 몫과 나머지를 분리한다.
남은 곱은 rate 분모와 proper fraction 분모의 u128 범위에 들어간다. inclusive와
strict 경계를 구분해 기존 음수 target scheduling을 보존하고, cue마다 binary
search나128단계 곱셈 대체 루프를 실행하지 않는다.

## 적용 결과의 소유권

기존 bounded practice ring과 output projection을 확장한다. 준비된 요청과 실제
첫 활성 PCM 프레임의 결정을 구분한다. 모든 voice/cursor를 먼저 검사해 함께
변경하며, 새 rate event는 원래 request/revision·정확한 cut·이전/요청/적용 rate와
거절/no-op을 보존한다. 기존 boundary receipt API는 opt-in typed event 접근으로
호환하고 표현할 수 없는 event를 조용히 버리지 않는다.

새 typed rate/restart는 실제 적용 순간의 generation/revision도 검사한다.
compound restart 거절의 boundary는 원래 cursor anchor의 무변경 marker로
requested/applied를 같게 두고 correction0을 사용한다. 원래 대상은 request id와
correlated pending 요청에 남는다. legacy applied region receipt의 보정 의미를
재사용해 발생하지 않은 seek 차이를 i64에 억지로 넣거나 공개 struct를 확장하지 않는다.

미래 BGM 상한은 현재 suffix뿐 아니라 반복될 전체 pass를 포함한다. 적용된
rate와 그 준비 상한을 동일 metadata sequence로 전달해, 이후 region admission도
현재 배속의 시간/용량을 bounded index 조회로 검사한다. 활성 Worklet 메시지에서
전체 cue를 다시 스캔하지 않는다.

Transport/capture/UI는 실제 owner-qualified 결과를 소비한다. 요청 전 history와
recording 용량을 확보하고, 실제 cut 뒤 모든 member candidate를 만든 다음 함께
commit한다. Browser의58-word output-report ABI는 유지하고 rate event를 별도의
typed 메시지로 전달한다. admission ACK를 실제 적용 ACK로 해석하지 않는다.

Worklet에는 시작 전에 retained Program을 설치해 future cue/end를 같은 phase로
계산한다. 재생 중 준비는 게임 Worker에서 수행한다. 전용 internal producer 포트의
Program/revision/geometry 검증은 분리된 인스턴스 사이의 소유권 계약이며 새로운
보안 인증 체계나 암호학적 attestation이 아니다.

기존 MessagePort의 제어 응답은 길이를 제한한 JS snapshot을 직렬화한다. Rust의
제어 적용은 준비된 scalar slot을 사용하고 `process()`는 기존 scalar render/copy를
유지한다. MessagePort 응답의 JS 복사·할당과 GC 비용까지 zero-cost라고 부르지 않는다.
실제 browser 검증에서 이 제어 비용과 출력 deadline을 확인해야 하며 Node spy나
Rust 무할당 검사만으로 AudioWorklet 전체의 무할당·무지연을 선언하지 않는다.

Browser의 이전 attempt는 기존 File 보관 흐름에서 Program/boundary/player로
구별한다. 최대128개·256MiB와 cohort당64MiB 예산으로 current export 공간을
예약하고 전체 cohort의 실제 인수 ACK 뒤에만 Worklet event를 해제한다. 자율 반복이
이 공간을 넘으면 명시적인 capacity 종료로 기존 데이터를 보존한다. 영구 저장은
기존 명시 Save의 성공 ACK를 따르며, 새 자동 저장 권한이나 무한 보관 큐는 추가하지 않는다.

## 대안과 결과

정수 anchor 또는 새 attempt는 작지만 PCM·score·fraction 연속성을 충족하지 않아
제외했다. 별도의 범용 timeline graph와 arbitrary-precision dependency는 현재
positive speed에 필요하지 않아 제외했다. 이미 admission된 고정 BGM queue를
전체 재작성하는 방식 대신 기존 retained Program을 재사용한다.

이 선택은 작은 순수 수학 객체와 bounded control 경로를 추가한다. 유한 분모
표현 용량이 sequence별 명시적인 거절 조건이 되고, exact Transport history와
가변 recording 이력이 실제 적용 경계만큼 증가한다. 기존 scalar Transport와
고정 배속의 bytes/동작은 유지한다. 실제 장치·browser acceptance는 별도로 남는다.
