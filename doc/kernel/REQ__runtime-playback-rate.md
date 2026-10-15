# 실행 중 양수 재생 배속 전환

상태: 구현 중. WBS05.13.06의 전체 기능 계약이며, 공통 수학 기반 구현만으로
지원 완료를 선언하지 않는다. 고정 배속은 [live](REQ__live-playback-speed.md)와
[retained](REQ__retained-playback-speed.md)를 따른다. 이 문서는 명시적으로
runtime 전환에 참여한 세션에만 추가 적용된다.

## 하나의 실제 적용 경계

배속은 정규화한 양수 i64/u64 유리수이며 pitch가 함께 바뀐다. 출력 장치 Hz,
buffer 크기, preroll 단위, 원본 입력 시각, audio epoch는 배속으로 바꾸지 않는다.
같은 attempt의 judge, score, gauge, held key와 발성 중인 모든 키음/BGM을 유지한다.
발성 중인 read-head를 정수 위치로 다시 시작하거나 새 attempt로 대체하지 않는다.

하나의 요청은 다음 실제 활성 PCM 프레임에서 모든 voice와 cue/end 시간에 함께
적용된다. startup/held/pause 무음 프레임은 적용 증거가 아니다. Pause 중 요청은
resume 뒤 첫 활성 프레임까지 pending이다. admission 성공, rendered event,
target projection, native presentation은 서로 다른 사실이다. UI는 실제 owner가
승인한 적용 결과만 표시하며 요청 시각으로 적용 프레임을 추정하지 않는다.

같은 정규화 배속 요청은 no-op이며 위상 segment와 기록 이력을 만들지 않는다.
한 번에 하나의 rate/region/restart 제어만 진행한다. stale generation/revision,
표현 범위·용량 부족, 종료/취소 경합은 상관관계를 보존한 거절로 답한다. 거절된
요청은 기존 PCM, rate, history, recording, opponent를 바꾸지 않는다.

## 공유하는 정확한 위상

E는 기존 output-phase의 누적 활성 출력 경과 ns이며 승인된 무음만 제외한다.
물리 프레임마다 따로 반올림한 경과 시간을 더하지 않는다. 원본 시작 S와
preroll P에 대해 처음에는 Q=-P*r0이고, 변경 경계에서는
Qcut=Qbase+(Ecut-Ebase)*old_rate다. 다음 segment는 이 Qcut의 분수까지 보존한다.
정수 곡 시간은 S+trunc(Q)이며 trunc(S+Q)로 바꾸지 않는다.

PCM cue/end 계산과 Transport는 같은 순수 phase 구현을 사용한다. 위상은
i128 정수와 정규화한 proper u64 분수로 보관하고, 연산 중 넓은 정수를 사용해
약분한 뒤 좁힌다. 다음 정수 E에서도 표현할 수 없는 denominator 조합은
별도 capacity 오류로 거절한다. 일반 Rate의 전역 허용 범위를 좁히지 않는다.

기록용 exact accessor는 uncapped S+Q의 floor 정수와 reduced proper 분수를
반환한다(분수가0이면0/1). 이는 표시용 S+trunc(Q)와 구분한다. S=100,P=1,
rate=1/2,E=0이면 정수 표시100, 정확한 원본 phase는99+1/2이다. 기록 이력을
정수 표시값에서 역으로 만들어 내지 않는다. 이 accessor는 실제 cut과 같이 Q를
i128으로 보관할 수 있는 범위에서 동작한다.

cue 역산은 기존 Program::frame_at 의미를 보존한다. T=target-S가0 이상이면
Q>=T인 첫 E를, T<0이면 Q>T-1인 첫 E를 사용한다. 이 구분은 preroll 중
정수 표시값이 시작에 닿았다는 이유로 BGM을 먼저 시작하지 않게 한다.
역산은 상수 개수의 정수 연산을 사용하며 cue마다 탐색하지 않는다.

과거 입력은 실제 적용 전 segment로 계속 매핑한다. 같은 정수 logical ns에
서로 다른 정확한 E가 있으면 두 값을 함께 사용해 경계를 구분한다. legacy
timestamp-only Transport 동작은 유지한다. Finite cap은 수치 상한이며 출력
완료·native ACK·input drain의 권한을 대신하지 않는다.

## 준비, callback과 소유권

전체 cue scan/sort, 저장 공간 확보와 미래 BGM overlap 준비는 오디오 렌더링
스레드 밖에서 수행한다. AudioWorklet의 onmessage도 이 스레드에 속한다.
callback은 모든 head와 cursor의 변경 가능성을 먼저 검사하고, 하나의 결정으로
함께 적용한다. callback과 활성 Worklet 메시지 경로에서 전체 채보 스캔, heap
할당/해제, 잠금, I/O를 새로 도입하지 않는다. 기존 명령 credit/ACK도 보존한다.

Browser는 arm 전에 원본 cue와 초기 구간으로 retained Program을 설치한다.
기존 고정 BgmFeeder와 이중으로 같은 cue를 예약하지 않는다. 게임 Worker가
불변 원본 cue/sample geometry에서 rate 준비를 수행하고 전용 제어 포트로
bounded 결과를 전달한다. Worklet은 원래 Program 식별자·revision·rate와 현재
용량을 검증한다. 내부 producer 포트의 준비 결과를 암호학적 증명으로 표현하지
않는다. 잘못된 숫자/shape는 panic이나 무제한 작업 대신 구체 오류로 거절한다.

Native Source/Target와 solo/local은 같은 business port를 사용한다. local cohort는
하나의 실제 cut/rate를 공유하고 모든 member의 commit을 준비한 뒤 반영한다.
요청 전에 capacity를 확보하되, 실제 event가 오기 전 예상 cut으로 Transport를
확정하지 않는다. backend 교체나 sample-rate 변경으로 배속을 흉내 내지 않는다.

## 구간 제어와 기록

scrub/loop는 현재 적용된 배속을 유지한다. F5는 기존 계약대로 원래 invocation의
start/end/선택 배속을 하나의 compound 경계에서 복원한다. region 변경과 rate
변경을 따로 승인해 중간 상태를 만들지 않는다.

실제 non-noop 전환만 원본 곡의 정확한 phase와 rate를 포함한 canonical 이력에
추가한다. 원래 배속으로 돌아와도 가변 배속 기록이다. 원본 replay operation과
입력 provenance는 바꾸지 않는다. NORMAL의 기존 무-envelope와 고정 v1 바이트는
보존하며 새로운 가변 envelope는 extent/count/order/fraction/version을 검증한다.

Capture 유무와 무관하게 completed result의 effective header가 실제 조건을
반영해야 한다. LiveReplayCapture의 header_bytes/encoded_bytes와 전체 file limit을
함께 검사·갱신한다. 변경 후 뒤늦은 allocation 실패로 이력이 누락되지 않도록
요청 전에 저장 용량을 확보한다. 원본 record를 재작성하는 generic recorder
framework를 추가하지 않고 app이 effective header를 소유한다.

pending 중 새 report도 예약한 header 증가분을 file limit에 포함한다. report별
codec 검사는 원래 고정 recorder header와 고정 base 길이를 재사용하고, 점점
길어지는 배속 이력 전체를 입력마다 clone/encode하지 않는다.

고정 조건으로 이미 admission된 saved/network 경쟁은 배속 전환을 이유와 함께
거절하고 상대 기록을 유지한다. 가변 배속 기록은 열기·리플레이·기록 조회가
가능해야 하며 고정 배속 상대인 것처럼 admission하지 않는다. 임의의 미래 가변
배속을 네트워크에서 합의하는 프로토콜은 별도 작업이다.

## UI와 검증 기준

기존 Play lifecycle에 붙는 작은 retained 선언형 제어를 사용한다. draft 오류는
입력값을 보존하고, pending은 실제 결과까지 유지하며, applied 값은 낙관적으로
변경하지 않는다. 키보드 focus와 Back/stop 복구가 가능해야 한다. Window는 입력과
gesture를 수집하고 business 준비·PCM·rendering은 Worker/Worklet이 담당한다.
VDOM이나 매 프레임 UI 재구성은 도입하지 않는다.

공통 phase의 양/음수 preroll, 분수 cut/no-op, 큰 정수·denominator·역산을 독립
literal/oracle로 검증한다. 실제 Mixer PCM, 여러 voice 중 마지막 실패의 전체
무변경, pause/resume, cue/end/loop 경계, converted suffix, calibrated callback
할당0, 실제 owner/recording/UI와 현재 WASM/browser 경로까지 확인한다. 순수 수학
테스트나 foreign typing은 전체 runtime 지원 또는 물리 음향/지연의 증거가 아니다.

## Known ceiling

Known ceiling: Exact accessor requires retained Q and resulting original-song floor to fit i128 — widen only if a concrete consumer requires phase beyond retained cut storage.
