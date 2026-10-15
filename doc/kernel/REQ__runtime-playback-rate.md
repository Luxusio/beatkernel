# 실행 중 양수 재생 배속 전환

상태: 구현 중. WBS05.13.06의 전체 기능 계약이며, 공통 수학 기반 구현만으로
지원 완료를 선언하지 않는다. 고정 배속은 [live](REQ__live-playback-speed.md)와
[retained](REQ__retained-playback-speed.md)를 따른다. 이 문서는 명시적으로
runtime 전환에 참여한 세션에만 추가 적용된다.

## 실제 시작 원점을 감싸는 브라우저 관측

Retained browser는 getOutputTimestamp의 실제 context/HOST 쌍에서 arm 원점만
뺀 signed output ns를 보존한다. 원점 직전의 음수 관측도 같은 실제 시계 이력에
남겨 Started의 source frame0을 실제 두 관측 사이에서 검증한다. 음수를0으로
자르거나 가짜0 관측을 삽입하지 않으며 boundary의 외삽을 허용하지 않는다.
음수 관측 자체는 playable output·Started 승인·입력 소비를 뜻하지 않는다.

공통 AudioAuthority의 명시 SourceTimestamp basis가 원본 signed ClockPair를
받고 기존 native SourceFrame/u64 SourceCounter 계약은 유지한다. 같은 정확한
분수 보간과 freshness·epoch·입력 prefix·제어 cutoff 검사를 사용한다. JS의
음수 허용은 retained 모드만 명시적으로 선택하며 일반/replay 의미는 유지한다.
예를 들어 원본 -5303245ns@1530920000ns와1459755ns@1537685000ns 사이의
source0에 처음 도달하는 정수 HOST는1536224814ns다. 그 전에는 제어 권한이나
Started ACK를 만들 수 없다.

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

전송 상관관계가 맞는 admission 거절 ACK를 처리한 사실과 재생 요청의 실패는
구분한다. 유효한 transaction ACK는 처리 성공으로 받고 원래 요청 ID의 typed
실패 응답을 보관한다. 잘못된 kind/ID나 중복 ACK는 기존 상태를 보존하고 거절한다.
실제 commit serial의 local ACK는 event latch만 해제하며, 이전 capture를 소비하거나
입력 history 회수를 허가하지 않는다. 남은 capture는 다음 fresh 경계의 service를
막고, 실제 Worker는 전원 회수 뒤 Worklet ACK를 보낸다.

샘플레이트 변환의 첫 Target sample 도달 증거와 실제 정수 Source 적용 cut은
분리한다. Rate 전용 native token은 원래 도달 증거를 보존하면서 실제 변환 이력의
정확한 source cut을 별도 query·logical/HOST·E로 검증한다. 이전 입력은 그 cut보다
앞까지만 처리하고 cut과 첫 Target sample 사이 입력은 새 배속으로 판정한다.
변환 span 밖으로 외삽하거나 E 일치를 완화하지 않으며, 기존 구간·F5 경계는
이 Rate 전용 보정의 대상이 아니다.

PCM에서 이미 발생한 Rate의 Source pin이 있지만 첫 Target projection은 아직
없는 exclusive block 끝에서는 해당 Source cut 이상 입력·frontier를 보존한다.
이미 검증한 실제 playback mapping의 정확한 Source 비교로 진행만 제한하며,
pin에서 HOST·위상·ACK나 native 권한을 만들어 내지 않는다. 단순 pending 요청은
이 제한을 만들지 않고, 기존 Pause/Resume·adoption 처리는 계속 진행한다.

같은 구간을 반복할 때는 다음 pass에서 다시 발성할 cue도 변경 용량 검사에
포함한다. 이후 scrub/loop의 시간 표현 범위와 BGM 용량도 현재 적용 배속으로
검사한다. 초기 선택 배속으로 유효한 요청을 거절하거나, 적용 뒤 실패를 반복하며
재생이 멈추는 상태로 진입해서는 안 된다.

빈 PCM을 참조하는 cue도 실행 작업 한도를 사용한다. 새 배속에서 cue가 같은
출력 프레임에 모여 최소 한 프레임조차 처리할 수 없게 되면 명시적인 작업 용량
거절로 기존 재생을 유지한다. 전체 블록이 너무 커서 생기는 기존 재시도 가능한
backpressure와 구분한다. Producer가 이 상한도 cold 준비하고 현재 배속과 함께
보존하며, 활성 Worklet에서 전체 cue를 다시 스캔하지 않는다.

거절된 typed compound restart의 boundary는 보존된 cursor anchor를 표시한다.
requested/applied anchor가 같고 correction은0이며 실제 frame·generation·iteration과
원래 request id를 유지한다. 원래 재시작 대상은 상관된 pending 요청에 보존한다.
거절 marker를 실제 seek나 새 attempt의 적용 결과로 처리하지 않는다. 따라서
적용하지 않은 두 대상의 차이가 i64를 넘는 경우에도 보정값을 자르지 않는다.

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

## 브라우저의 전체 voice 예산과 BGM 예약

실제 Mixer의 전체 maxVoices와 retained BGM 전용 max_overlap 예약은 구분한다.
원본 전체 cue geometry와 초기 선택 배속의 기존 exact overlap 계산을 재사용해
BGM 예약분을 정하고, 나머지 전체 슬롯은 모든 local member의 키음이 공유한다.
처음 선택한 구간이나 cue 개수, 전체 voice 한도 자체를 BGM 예약량으로 쓰지 않는다.
무BGM·빈 PCM의 overlap0은 유효하며 실제 필요한 두 풀이 전체 예산에 들어가지
않으면 cold 단계에서 기존 상태를 보존하고 구체적으로 거절한다. Cold 최소 키음
예산은 기존 native와 동일하게 원본 notes/invisible/mines가 있으면 공유 슬롯1개
이상을 요구한다. local 인원수만큼 별도 슬롯을 강제하지 않으며, BGM-only는
키음 슬롯0개도 허용한다.

향후 배속·구간 변경이 예약된 BGM 예산을 실제로 넘으면 기존 capacity 거절로
재생과 기록을 보존한다. 전체 용량을 몰래 늘리거나 키음을 훔치거나 버리지 않는다.
명시 PracticeLimits를 받는 공통 Step API와 기존 native 예산 정책은 유지한다.
Browser의 실제 total을 받는 cold adapter만 공유 계산으로 정확히 분할하며 전체
geometry의 추가 중복 scan/sort나 오디오 스레드 작업을 새로 만들지 않는다.

## 준비, callback과 소유권

전체 cue scan/sort, 저장 공간 확보와 미래 BGM overlap 준비는 오디오 렌더링
스레드 밖에서 수행한다. AudioWorklet의 onmessage도 이 스레드에 속한다.
callback은 모든 head와 cursor의 변경 가능성을 먼저 검사하고, 하나의 결정으로
함께 적용한다. callback과 활성 Worklet의 Rust 제어 적용에는 전체 채보 스캔,
heap 할당/해제, 잠금, I/O를 새로 도입하지 않는다. 기존 MessagePort 제어 응답의
직렬화에 필요한 bounded JS snapshot 복사는 별도 비용으로 명시하고 render의
`process()` 밖에서만 수행한다. 이 복사까지 무할당이라고 주장하지 않으며 기존
명령 credit/ACK도 보존한다.

Browser는 arm 전에 원본 cue와 초기 구간으로 retained Program을 설치한다.
준비 job의 첫 advance 전에 retained 모드를 명시적으로 선택한다. 이 모드에서는
초기 judge의 구간만 선택하고 원본 채보·sample ID·PCM bank·sound binding·전체
BGM cue를 보존한다. 뒤로 이동하거나 F5를 실행할 때 이미 잘린 PCM suffix에서
복원하지 않는다. 이미 진행·완료·취소·인계된 job이나 recorded replay 준비를
나중에 retained live 준비로 바꾸지 않는다. 기존 일반·replay 준비 경로는 유지한다.
기존 고정 BgmFeeder와 이중으로 같은 cue를 예약하지 않는다. 게임 Worker가
불변 원본 cue/sample geometry에서 rate 준비를 수행하고 전용 제어 포트로
bounded 결과를 전달한다. Worklet은 원래 Program 식별자·revision·rate와 현재
용량을 검증한다. 내부 producer 포트의 준비 결과를 암호학적 증명으로 표현하지
않는다. 잘못된 숫자/shape는 panic이나 무제한 작업 대신 구체 오류로 거절한다.

Native Source/Target와 solo/local은 같은 business port를 사용한다. local cohort는
하나의 실제 cut/rate를 공유하고 모든 member의 commit을 준비한 뒤 반영한다.
원시 출력 domain과 기록용 normalized logical domain은 서로 다를 수 있다.
member들의 header는 같은 logical domain을 사용하고 실제 Transport basis와
native proof의 logical output으로 검증한다. Mixer의 raw origin과 같다고 가정하지
않으며 잘못된 domain의 준비는 기존 owner와 pending 요청을 보존한 채 거절한다.
요청 전에 capacity를 확보하되, 실제 event가 오기 전 예상 cut으로 Transport를
확정하지 않는다. backend 교체나 sample-rate 변경으로 배속을 흉내 내지 않는다.

Native runtime owner는 첫 PCM 렌더 전에 원래 header·capture 예약을 갖춘 상태로
생성한다. Worklet projection은 내부 상대 원점(domain1,0ns)을 유지하고, 외부 RAW
시계 원점이나 normalized LOGICAL 원점과 동일시하지 않는다. 상대 target frame/time과
실제 source Hz를 검증한 뒤 기존 authority로 각 원점을 연결한다.

Browser retained 관측은 기존 getOutputTimestamp에서 얻은 signed audio-ns/host-ns
쌍을 명시 SourceTimestamp basis로 그대로 보존한다. 원점 전 실제 관측도 유지한다. ns를 물리 sample frame으로 역산해
관측한 값처럼 표시하지 않는다. AudioContext.currentTime은 미래 arm 선택용이며
presentation을 대신하지 않는다. 입력 authority의 실제 future-input 하한과 대기
event의 최소 physical cut을 별도 history pin으로 전달하고 Worklet의 queued/latch
pin도 따로 검사한다. event ACK나 최신 rendered frontier로 pin을 추정하지 않는다.

실제 합의된 Mixer voice/event 한도와 bounded 제어·receipt·history 용량은 초기
생성 직후 설정한다. 활성화·관측·녹화·요청·event가 시작된 뒤 바꾸지 않으며,
초기 기본값을 실제 장치 설정으로 가정하지 않는다. Worklet은 업로드된 설정을
자신의 실제 Mixer 한도와 다시 대조하고 지원 불가한 시작을 명시적으로 거절한다.

원본 채보의 전체 lane/화면 geometry는 처음 구간을 고르기 전에 기존 compiled
결과에서 준비한다. 구간별 화면은 원본 geometry에서 head가 시작 전인 노트를
제외하고, 교차 hold 전체 제외와 원래 lane index를 유지한다. 새 구간의 chart와
모든 member의 NoteProgress는 실제 commit 전에 준비하고 일치하는 serial에서만
설치한다. Worker는 새 visual content를 등록한 뒤 frame을 전송한다. Rate/Pause나
거절된 제어를 이유로 기존 note/held/score 진행을 초기화하지 않는다.

## 구간 제어와 기록

scrub/loop는 현재 적용된 배속을 유지한다. F5는 기존 계약대로 원래 invocation의
start/end/선택 배속을 하나의 compound 경계에서 복원한다. region 변경과 rate
변경을 따로 승인해 중간 상태를 만들지 않는다.

RuntimeGroup과 SoloRuntime의 일반 구간 교체는 초기 anchor가 아닌 현재 적용된
runtime 배속을 보존하고 fixed/runtime 모드를 섞지 않는다. 서로 다른 배속의 새
attempt는 실제 compound original-restart를 확인한 전용 경로에서만 설치한다.
이때도 원래 출력 owner/domain/sample grid, 증가한 generation, 새 judge와 원본
object/profile 검증을 유지한다. fresh business는 실제 누적 revision을 이어받는다.

Browser의 fresh 구간 경계는 이전 모든 member의 accepted capture를 하나의
bounded retired-cohort slot으로 이동한다. Worker가 기존 기록 저장 흐름으로
전원을 회수하기 전에는 다음 fresh 경계를 확정·ACK하지 않는다. 현재 capture의
기존 stopped export 의미는 유지하며, 이미 종료된 attempt의 별도 export로
이전 prefix를 회수한다. 종료·오류에서도 남은 prefix를 버리지 않고, 미완료
prefix를 완료 결과로 표시하지 않는다. Rate 변경 자체는 attempt를 종료하지 않는다.

Browser는 기존 captured File 목록을 최대128개·256MiB로 제한하고 전송 중인
retired cohort는 하나만 보유한다. 기존 cohort당64MiB와 실제 roster 수를 사용해
새 fresh 요청 전에 이전 cohort와 다음 current export 공간을 함께 남겨 둔다.
공간 부족은 명시적 요청이면 enqueue 전 거절하고, 자율 반복이면 business commit
전에 capacity 오류로 종료해 이전 File과 current capture를 보존한다. 자동 DB 저장이나
download는 하지 않는다. 명시 Save의 실제 저장 성공 ACK 뒤에만 해당 임시 보관을
해제할 수 있다. Capture-off는 File 보관 공간을 사용하지 않는다.

보관 키는 실제 Program·fresh boundary serial·player다. 종료 시 current capture는
별도 null 키로 구분한다. Window는 전체 roster·File 크기·예산을 검사하고 전원을
보관한 뒤에만 retired acceptance ACK를 보낸다. Worker는 그 ACK까지 File을 보유하고
이후 Worklet ACK→Rust ACK를 진행한다. 미완료 prefix의 score는 null이고 현재
attempt의 score나 completed archive를 붙이지 않는다. 종료·실패에서도 보관을 마친
뒤 Worker 소유권을 해제하며, 아직 주인 없는 capture가 있으면 released를 보고하지 않는다.
current 종료 응답의 File·metadata 검증에 실패하면 원본 immutable File 응답을
보존하고 보관 승인과 Worker 해제를 보류한다. 실패한 응답을 빈 기록 목록의
성공으로 바꾸거나, 기존 retired 기록과 current 기록의 보관 키를 합치지 않는다.

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

실제 event를 받은 뒤 준비 검사가 실패하거나 준비 payload를 폐기해도 원래
pending 예약은 commit·거절까지 유지한다. 그 사이 새 report가 예약된 공간을
차지하거나, 실제 PCM 배속은 바뀌었는데 기록 이력을 확정하지 못하면 안 된다.
Cold 준비 token의 복제본은 하나의 예약을 공유하고 첫 commit 또는 명시 취소가
다른 복제본을 무효화한다. 마지막 미확정 token을 폐기하면 예약이 해제된다.

고정 조건으로 이미 admission된 saved/network 경쟁은 배속 전환을 이유와 함께
거절하고 상대 기록을 유지한다. 가변 배속 기록은 열기·리플레이·기록 조회가
가능해야 하며 고정 배속 상대인 것처럼 admission하지 않는다. 임의의 미래 가변
배속을 네트워크에서 합의하는 프로토콜은 별도 작업이다.

현재 설정과 비교하는 `RecordPreview::inspect`는 기존 seed·판정 class·timing·gauge·
profile·구간·입력 방식·고정 배속 일치를 계속 검사한다. 유효한 가변 조건의 역사
조회는 명시적인 `from_file_for_history`를 사용하며, 조회 성공이 고정 상대의
호환성 승인이 되지 않는다.

## UI와 검증 기준

기존 Play lifecycle에 붙는 작은 retained 선언형 제어를 사용한다. draft 오류는
입력값을 보존하고, pending은 실제 결과까지 유지하며, applied 값은 낙관적으로
변경하지 않는다. 키보드 focus와 Back/stop 복구가 가능해야 한다. Window는 입력과
gesture를 수집하고 business 준비·PCM·rendering은 Worker/Worklet이 담당한다.
VDOM이나 매 프레임 UI 재구성은 도입하지 않는다.

Native Play의 배속 편집 focus는 처음에 없다. Tab은 분자·분모·Apply·focus 해제
순서로 이동하고 Shift+Tab은 역순이다. 편집기 또는 Apply focus에서 Enter로 현재
draft를 제출하며, 편집기 클릭은 해당 입력을 선택한다. 커서·삭제·IME·clipboard는
편집기에 focus가 있을 때만 전달한다. Escape/F5/F9의 기존 Play 제어는 유지한다.
pending 중에도 draft와 실제 적용값을 보존하며 Apply를 중복 실행하지 않는다.

Browser의 admission 성공은 pending 응답으로 Window의 기존 RPC 슬롯만 해제한다.
실제 business pending은 유지하고 적용 결과는 원래 요청 ID로 따로 전달한다.
따라서 pause 중 배속 요청이 있어도 resume 요청을 보낼 수 있다. 실제 owner의
commit 전에는 적용값·노트 상태·완료 결과를 낙관적으로 바꾸지 않는다.

시작 전 실제 capability는 없으며 준비 응답의 capability는 null이다. 기존
render-done의 retainedCapability는 실제 owner의16word view 또는 null을 전달한다.
실제 Started 이후에만 제어를 활성화하고 Ended 이후에는 권한을 철회한다.
이는 원래 설정이나 generation 메타데이터와 다르며, 시작 증거를 만들어 내지 않는다.
Ended 뒤 도착한 pending Rate 거절의 실제 결과도 capability:null을 허용한다.

fresh attempt는 실제 Requested·Looped·성공한 compound restart만 뜻한다.
Started·Ended·Rate·Pause·거절은 generation 변화만으로 새 attempt가 되지 않는다.
특히 Ended의 generation 증가 때문에 최종 current capture를 이전 cohort로 옮기거나
노트·점수·held 상태를 초기화하지 않는다. generation이 필요하면 실제 business가
matching serial을 commit한 원래 event에서 읽는다.

공통 phase의 양/음수 preroll, 분수 cut/no-op, 큰 정수·denominator·역산을 독립
literal/oracle로 검증한다. 실제 Mixer PCM, 여러 voice 중 마지막 실패의 전체
무변경, pause/resume, cue/end/loop 경계, converted suffix, calibrated callback
할당0, 실제 owner/recording/UI와 현재 WASM/browser 경로까지 확인한다. 순수 수학
테스트나 foreign typing은 전체 runtime 지원 또는 물리 음향/지연의 증거가 아니다.

## Known ceiling

Known ceiling: Exact accessor requires retained Q and resulting original-song floor to fit i128 — widen only if a concrete consumer requires phase beyond retained cut storage.

Source1000→Target1500의 실제 raw Source17/미투영 Target26 경계는 공통 solo와
2·4인 cohort의 capture 활성·비활성 회귀로 확인했다. 해당 입력을 보존하고 실제
projection 후 새 배속으로 처리한다. 이 supplied-clock 소프트웨어 증거가 실제
driver·장치 acceptance를 대신하지는 않는다.
