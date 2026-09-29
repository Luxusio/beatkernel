**BeatKernel**

**범용 저지연 리듬게임 커널 설계 및 AI 개발 실행 명세**

Rust · Ultra-low-latency Input · Deterministic Timing · Judge · Visual
Projection · Real-time Audio

**Version 0.2 — Architecture & Implementation Plan**

2026-09-29

# **0. 문서 목적과 사용법**

이 문서는 `BeatKernel`이라는 Rust 기반 범용 리듬게임 런타임을 구현하기 위한 설계 및 실행 명세다. 특정 플레이어 하나를 복제하는 것이 아니라, BMS/IIDX 계열부터 osu!, Arcaea형 멀티터치, SDVX형 축 입력, CHUNITHM/maimai형 공간·터치 입력, GITADORA형 복합 입력, Taiko형 반복 입력, VR 리듬게임까지 확장 가능한 공통 기반을 목표로 한다.

주요 독자는 AI 코딩 에이전트와 인간 개발자다. 구현자는 이 문서의 불변식, 계층 경계, 의존성 방향, 단계별 완료 조건을 우선해야 한다. 논리적 모듈을 무조건 별도 crate로 분리하지 않으며, 실제 독립 배포·플랫폼 격리·재사용 이유가 있을 때만 crate 경계를 만든다.

| **핵심 목표** BeatKernel 자체는 완성형 렌더링 엔진이 아니다. OS/장치 입력 정규화, 공통 시간축, 채보 컴파일, 상호작용/판정, 렌더 상태 계산, 초저지연 오디오 스케줄링, 리플레이와 seek/reverse를 제공하는 리듬게임용 runtime kernel이다. |
|---|

# **1. 제품 정의**

## **1.1 한 문장 정의**

“여러 OS와 장치의 입력을 공통 물리 입력 체계와 정밀한 시간축으로 정규화하고, 게임별 매핑·상호작용 규칙에 따라 결정론적으로 판정하며, 같은 시간축을 기준으로 시각 상태와 초저지연 오디오를 스케줄링하는 범용 리듬게임 커널.”

## **1.2 반드시 해결할 문제**

- Windows/macOS/Linux의 native/raw 입력을 가능한 한 원본 timestamp와 장치 identity를 보존해 획득한다.
- 서로 다른 OS의 키보드 코드가 같은 물리 키라면 하나의 canonical physical key ID로 정규화한다.
- 동일한 물리 키라도 서로 다른 장치에서 온 입력은 `DeviceId + PhysicalControlId`로 구분한다.
- 커스텀 HID/아케이드 장치는 raw report를 장치별 adapter로 해석할 수 있어야 한다.
- Physical Input과 Game Control의 binding을 분리해 게임 코드가 Windows scan code, Linux evdev code 등을 알지 않게 한다.
- BPM/STOP/SV/특수 타이밍을 런타임에 적합한 절대 시간축으로 컴파일한다.
- 판정 규칙과 판정 윈도우를 게임별로 교체할 수 있게 한다.
- 렌더러가 “현재 무엇을 어디에 그려야 하는지” 계산된 논리 상태를 받을 수 있게 한다.
- 오디오 callback에서 allocation/lock/I/O 없이 sample-accurate scheduling을 수행한다.
- pause, seek, 실시간 배속, 역재생을 Transport 계층에서 처리한다.
- 동일 Chart + Rules + Options + Input Replay가 동일 결과를 내도록 결정론을 보장한다.
- 플랫폼 구현, 게임 규칙, 장치 어댑터가 서로 역방향으로 침범하지 않도록 의존성 방향을 고정한다.

## **1.3 비목표(초기 버전)**

- 완성형 2D/3D 렌더링 엔진 자체를 제공하지 않는다.
- Unity/Godot/Bevy 수준의 scene editor, asset pipeline, physics engine을 제공하지 않는다.
- 모든 리듬게임 규칙을 처음부터 하나의 DSL/VM으로 표현하려 하지 않는다.
- 내부 논리 모듈을 전부 독립 crate로 쪼개지 않는다.
- 초기 단계에서 모든 OS/모든 아케이드 컨트롤러를 동시에 지원하지 않는다.
- 초기 단계에서 네트워크 IR, 계정/랭킹/멀티플레이를 구현하지 않는다.

# **2. 설계 원칙과 불변식**

| **ID** | **원칙** | **설명** |
|---|---|---|
| P1 | 시간과 의미를 분리 | OS/장치의 의미는 보존하되 timing/order/replay는 공통 메타데이터로 처리한다. |
| P2 | 판정과 시각화를 분리 | 배속/SV/역스크롤이 판정 결과에 영향을 주지 않도록 Judge Timeline과 Visual Projection을 분리한다. |
| P3 | 벽시계와 곡 시간을 분리 | Transport가 host time ↔ song time 매핑을 소유하며 rate/pause/seek/reverse를 piecewise mapping으로 처리한다. |
| P4 | 오디오 RT 경계 엄수 | Audio callback에서 heap allocation, blocking lock, disk I/O, decoding, logging을 금지한다. |
| P5 | 결정론 | 판정 핵심 시간은 integer/fixed-point 기반으로 두고 replay 결과가 플랫폼 차이로 흔들리지 않도록 한다. |
| P6 | 과도한 추상화 금지 | 모든 입력을 익명 float 배열로 만들지 않는다. Button/Axis/Touch/Pointer/Pose 같은 의미형 입력을 유지한다. |
| P7 | 코어는 OS와 게임을 몰라야 함 | `beatkernel`에는 Windows API, evdev, IOHID, IIDX/SDVX/Arcaea 이름을 하드코딩하지 않는다. |
| P8 | 물리 키 identity 표준화 | 키보드는 가능한 한 USB HID Keyboard/Keypad Usage 기반 canonical ID로 통일한다. 원본 native code는 메타데이터로 보존한다. |
| P9 | 의존성은 composition root에서 만난다 | 플랫폼 구현과 게임 어댑터는 서로 의존하지 않는다. 최종 app/example만 둘을 조립한다. |
| P10 | crate보다 module을 우선 | 독립 배포/플랫폼 격리/별도 의존성 이유가 없는 논리 단위는 `beatkernel` 내부 module로 둔다. |
| P11 | 측정 가능성 | latency/jitter/underrun/order/clock mapping 품질을 추적할 telemetry와 benchmark를 처음부터 제공한다. |

# **3. 전체 계층 구조**

```text
Physical Device
  Keyboard / HID / Touch / Gamepad / Arcade IO / VR / Sensors
            |
            v
OS Acquisition Backend                         [beatkernel-platform]
  Windows: Raw Input / HID / GameInput
  Linux:   evdev / hidraw / optional libusb
  macOS:   IOHIDManager / native input APIs
  + native timestamp / device discovery / raw metadata
            |
            v
Native / Raw Input
            |
       +----+---------------------------+
       |                                |
       v                                v
Standard Normalizer                Device Adapter (optional)
  scan code/evdev/HID usage         custom HID report parser
  -> canonical control              calibration/range decode
       |                                |
       +---------------+----------------+
                       v
Canonical Physical Input                     [beatkernel::input]
  DeviceId + PhysicalControlId + timestamp
  Button / Axis / Touch / Pointer / Pose / Custom
                       |
                       v
Input Mapping / Binding                       [beatkernel::input::binding]
  physical control -> game control/channel
  original physical source metadata retained
                       |
                       v
Game Semantic Input                           [game adapter/profile]
                       |
             +---------+----------+
             |                    |
             v                    v
       BeatKernel Runtime      Replay Recorder
  Clock / Transport / Chart / Interaction / Judge
             |
       +-----+------------------+
       |                        |
       v                        v
  Render State             Audio Commands
       |                        |
External Renderer       RT Audio Scheduler
                                |
                                v
                         Platform Audio Output
```

의존성 방향은 다음을 고정한다.

```text
                 beatkernel
                 ^       ^
                 |       |
 beatkernel-platform     game adapter (예: beatkernel-bms)
                 ^       ^
                  \     /
                   \   /
                 final app
```

`beatkernel-platform`은 `beatkernel`의 공통 타입/trait를 구현한다. 게임 어댑터도 `beatkernel`만 의존한다. 플랫폼 crate와 게임 어댑터는 서로를 모른다. Windows/macOS/Linux 중 무엇을 사용할지는 최종 app의 composition root가 선택한다.

# **4. Rust Workspace 구조**

초기에는 핵심을 과하게 crate로 분해하지 않는다. MVP 기준 실제 핵심 crate는 `beatkernel`과 `beatkernel-platform` 두 개면 충분하다.

```text
beatkernel/
├─ Cargo.toml                    # workspace
├─ crates/
│  ├─ beatkernel/                # OS 독립 리듬게임 커널
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ time/
│  │     ├─ transport/
│  │     ├─ input/
│  │     │  ├─ device.rs
│  │     │  ├─ physical.rs
│  │     │  ├─ key.rs
│  │     │  ├─ binding.rs
│  │     │  └─ event.rs
│  │     ├─ chart/
│  │     │  ├─ source.rs
│  │     │  └─ compiled.rs
│  │     ├─ interaction/
│  │     ├─ judge/
│  │     ├─ visual/
│  │     ├─ audio/
│  │     ├─ replay/
│  │     ├─ runtime/
│  │     └─ telemetry/
│  │
│  └─ beatkernel-platform/       # target별 native input/audio 구현
│     └─ src/
│        ├─ lib.rs
│        ├─ windows/
│        ├─ linux/
│        └─ macos/
│
├─ adapters/                     # 필요해질 때만 별도 crate로 승격
│  ├─ beatkernel-bms/
│  ├─ beatkernel-osu/
│  └─ beatkernel-devices/        # 알려진 아케이드/HID adapter 모음(선택)
│
├─ apps/
│  ├─ input-inspector/
│  ├─ latency-bench/
│  └─ beatkernel-demo/
│
└─ docs/
```

| **crate/module** | **책임** | **비고** |
|---|---|---|
| `beatkernel` | time, transport, canonical input, binding, chart compile, interaction, judge, visual state, audio scheduler/mixer core, replay, runtime | OS API를 직접 의존하지 않는다. |
| `beatkernel-platform` | Windows/Linux/macOS native input, HID/raw backend, audio device backend, native clock bridge | target-specific dependency와 `cfg`를 사용한다. |
| `beatkernel-bms` 등 | 파일 parser, 게임 규칙/profile, 게임 control catalog, game-specific projection/policy | 플랫폼 crate를 의존하지 않는다. |
| final app | 플랫폼 backend + 게임 어댑터 + renderer를 조립 | 유일한 composition root다. |

### **4.1 target-specific dependency 원칙**

`beatkernel-platform` 하나를 의존해도 빌드 대상이 아닌 OS 구현을 컴파일할 필요는 없다.

```toml
[dependencies]
beatkernel = { path = "../beatkernel" }

[target.'cfg(target_os = "windows")'.dependencies]
windows = "..."

[target.'cfg(target_os = "linux")'.dependencies]
# evdev/hidraw/PipeWire/ALSA 관련 의존성

[target.'cfg(target_os = "macos")'.dependencies]
# IOHID/CoreAudio 관련 의존성
```

외부 API에서는 필요하면 `DefaultInputBackend`, `DefaultAudioBackend` 같은 target-specific alias를 노출한다. 게임 어댑터가 세 OS crate를 다시 의존하는 구조는 금지한다.

### **4.2 crate 분리 기준**

다음 중 하나가 실제로 생겼을 때만 새 crate를 만든다.

- OS/FFI 의존성을 격리해야 한다.
- 다른 프로젝트가 독립적으로 재사용할 가치가 있다.
- 독립 버전/배포가 필요하다.
- 빌드 feature/의존성 무게를 실제로 줄일 수 있다.

`time`, `judge`, `ir`, `transport`, `runtime`이라는 이름이 논리적으로 존재한다는 이유만으로 별도 crate를 만들지 않는다.

# **5. 시간 모델**

## **5.1 기본 타입**

\#\[repr(transparent)\]  
pub struct Timestamp(i64); // runtime 기준 정수 tick, 기본 제안: ns  
  
\#\[repr(transparent)\]  
pub struct Duration(i64);  
  
pub struct ClockDomainId(u32);  
  
pub struct ClockPoint {  
pub domain: ClockDomainId,  
pub timestamp: Timestamp,  
}

판정 핵심 경로에서 f32/f64 seconds를 기준 시간으로 사용하지 않는다.
실수는 시각 보간이나 DSP 등 필요한 곳에서만 사용한다.

## **5.2 Clock Domain**

입력 OS timestamp, host monotonic clock, audio device clock은 동일하다고
가정하지 않는다. 각 clock domain 사이의 mapping을 명시적으로 관리한다.

pub trait ClockMapper {  
fn map(&self, from: ClockPoint, to: ClockDomainId) -\>
Option\<Timestamp\>;  
fn quality(&self) -\> ClockMappingQuality;  
}

## **5.3 Transport**

pub struct TransportAnchor {  
pub host_time: Timestamp,  
pub song_time: Timestamp,  
pub rate: Rate,  
}  
  
// anchor 이후 구간  
song_time = anchor.song_time  
+ (host_time - anchor.host_time) \* anchor.rate

- rate = 1.0: 정상 재생

- rate = 0.5: 반속

- rate = 0.0: 정지

- rate \< 0: 역재생

- 실시간 rate 변경 시 현재 위치에서 새 anchor를 생성한다.

- seek는 새 song_time을 가진 anchor를 생성하며 과거 rate로 전체
  경과시간을 다시 곱하지 않는다.

# **6. 입력 시스템**

입력 시스템은 “OS raw code를 게임 action으로 바로 변환”하지 않는다. 장치 identity와 물리 control identity를 보존한 채 단계적으로 정규화한다.

## **6.1 공통 메타데이터와 장치 identity**

```rust
pub struct EventMeta {
    pub source: DeviceId,
    pub timestamp: Timestamp,
    pub clock_domain: ClockDomainId,
    pub sequence: u64,
    pub native: Option<NativeEventMeta>,
}

pub struct DeviceDescriptor {
    pub runtime_id: DeviceId,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub serial: Option<String>,
    pub name: Option<String>,
    pub transport: DeviceTransport,
    pub capabilities: DeviceCapabilities,
}
```

`DeviceId`는 현재 실행에서 장치를 확실히 구분하는 runtime identity다. 설정 저장 시 같은 장치를 다시 찾기 위해 VID/PID/serial/path 등의 fingerprint를 사용할 수 있지만, 모든 키보드가 serial을 제공하지 않으므로 영구적으로 완벽한 ID라고 가정하지 않는다.

## **6.2 Canonical Physical Control**

키보드 물리 키는 가능한 한 USB HID Keyboard/Keypad Usage ID를 canonical representation으로 사용한다. Windows scan code, Linux evdev key code, macOS HID usage를 backend에서 이 공통 ID로 변환한다.

```rust
pub enum PhysicalControlId {
    HidUsage {
        usage_page: u16,
        usage: u16,
    },
    Native {
        backend: BackendId,
        code: u32,
    },
    Vendor {
        namespace: VendorNamespaceId,
        code: u32,
    },
}
```

표준 키보드의 `A` 물리 키가 어느 OS에서 들어오든 가능한 경우 동일한 HID Usage 기반 `PhysicalControlId`가 된다. 반면 OS/장치가 표준 HID 의미로 손실 없이 변환되지 않는 경우 `Native` 또는 장치 adapter의 `Vendor` control을 사용해 정보를 버리지 않는다.

문자 `"A"`, `"a"`, `"ㅁ"` 같은 logical/text 입력은 gameplay physical binding의 기준이 아니다. 텍스트 입력이 필요한 UI는 별도 text input 계층을 사용한다.

## **6.3 Canonical Physical Input Event**

```rust
pub enum PhysicalInputEvent {
    Button(ButtonEvent),
    Axis(AxisEvent),
    Touch(TouchEvent),
    Pointer(PointerEvent),
    Pose(PoseEvent),
    RawHidReport(RawHidReportEvent),
    Custom(CustomInputEvent),
}

pub struct ButtonEvent {
    pub meta: EventMeta,
    pub control: PhysicalControlId,
    pub state: ButtonState,
}

pub struct AxisEvent {
    pub meta: EventMeta,
    pub control: PhysicalControlId,
    pub value: f32,
    pub mode: AxisMode,
}
```

같은 물리 키라도 `DeviceId`가 다르면 다른 입력이다. 따라서 키보드 두 대에서 같은 `Keyboard A`가 들어와도 다음처럼 구분된다.

```text
Device #17 / Keyboard-A / Down
Device #24 / Keyboard-A / Down
```

## **6.4 Raw Backend와 Device Adapter 경계**

OS backend의 책임:

- 장치 발견/연결/해제
- native event 또는 raw HID report 읽기
- 가능한 가장 원본에 가까운 timestamp와 ordering 획득
- 표준 장치의 native control code를 canonical HID/control ID로 변환
- 필요 시 raw/native metadata 보존

Device Adapter의 책임:

- 커스텀 VID/PID/report descriptor로 장치를 식별
- raw HID report 의미를 해석
- 장치 특화 calibration/range/rotary delta를 canonical physical event로 변환
- 표준화할 수 없는 control은 vendor namespace를 사용

`beatkernel`의 책임:

- Windows `RAWINPUT`, Linux `input_event`, macOS native struct 자체를 알지 않는다.
- adapter interface와 canonical event model만 제공한다.

```rust
pub trait DeviceAdapter {
    fn accepts(&self, device: &DeviceDescriptor) -> bool;
    fn on_report(
        &mut self,
        report: &RawHidReportEvent,
        out: &mut dyn PhysicalInputSink,
    );
}
```

## **6.5 Binding: Physical Input → Game Control**

게임 코드는 물리 키나 OS key code를 직접 사용하지 않는다. 사용자 설정의 binding layer가 물리 control을 게임이 정의한 논리 control/channel로 매핑한다.

```rust
pub struct Binding {
    pub device: DeviceSelector,
    pub physical: PhysicalControlId,
    pub game_control: GameControlId,
}
```

예시:

```text
Keyboard #1 / Z                 -> IIDX_KEY_1
Keyboard #1 / S                 -> IIDX_KEY_2
Phoenixwan / HID Button 0       -> IIDX_KEY_1
Phoenixwan / Turntable Axis     -> IIDX_SCRATCH
```

게임 어댑터는 `IIDX_KEY_1` 같은 game control만 알며 Windows/Linux/macOS key code를 알지 않는다. Binding 결과에도 원본 `DeviceId`와 physical source를 남겨 replay/debug/latency 분석에 사용할 수 있게 한다.

Touch/Pointer/Pose처럼 위치나 contact identity 자체가 게임 규칙에 필요한 스트림은 단순 버튼 action으로 축소하지 않고, 게임이 등록한 logical surface/channel에 매핑한 뒤 원래 payload를 보존한다.

## **6.6 Touch Contact Identity**

멀티터치에서는 위치 값뿐 아니라 contact identity와 lifecycle이 중요하다. `Down → Move* → Up/Cancel`을 같은 `ContactId`로 추적한다.

```text
Touch #17 Down @ (x0,y0)
Touch #17 Move @ (x1,y1)
Touch #17 Move @ (x2,y2)
Touch #17 Up

Touch #18 = 독립 trajectory
```

Arcaea형 연속 터치나 maimai형 slide에서 최초 contact 고정, 재접촉 허용, 다른 contact로 rebind 등의 정책은 Interaction/Game Policy가 결정하며 input backend가 임의로 손가락 identity를 합치지 않는다.

## **6.7 Replay 기준 입력**

Replay는 가능하면 OS-native raw bytes가 아니라 clock-normalized canonical physical/game input을 기록한다. 디버그 모드에서는 native metadata/raw report를 별도 trace로 저장할 수 있다. 이렇게 해야 플랫폼이 달라도 동일 게임 규칙을 재검증하기 쉽다.

# **7. Chart Model과 컴파일**

## **7.1 Source Chart와 Compiled Chart 분리**

파일 포맷의 의미를 가능한 한 보존하는 Source Chart와, 플레이 중 빠르게 조회하는 Compiled Chart를 분리한다. 둘은 `beatkernel::chart` 내부 논리 모듈이며 별도 crate가 아니다.

BMS / osu! / custom chart / game-specific source  
\|  
v  
Source Chart  
beat, BPM, stop, SV, object definitions  
\|  
Timeline Compiler  
\|  
v  
Compiled Chart  
absolute timestamps + precompiled lookup data

## **7.2 Compiled TimedObject**

pub struct TimedObject {  
pub id: ObjectId,  
pub time: TimeRange,  
pub interaction: InteractionId,  
pub visual: VisualId,  
pub audio: Option\<AudioBinding\>,  
pub metadata: ObjectMetadata,  
}

## **7.3 판정 시간과 Visual Timeline 분리**

BPM/SV/scroll gimmick에 의해 화면 이동이 바뀌어도 note의 judge target
time은 변하지 않아야 한다. Visual Timeline은 별도의 projection 데이터로
유지한다.

# **8. Interaction Model**

“Note 종류”를 무한히 늘리는 대신 입력과 시간 관계의 기본 interaction
primitive를 제공한다. 게임별 특수 규칙은 Rust trait/custom evaluator로
확장한다.

| **Primitive** | **의미**                                           | **대표 사례**                          |
|---------------|----------------------------------------------------|----------------------------------------|
| Instant       | 특정 시간 근처의 순간 입력                         | IIDX tap, Taiko hit, Muse Dash         |
| Hold          | 시작/유지/종료 상태                                | LN, 일반 hold                          |
| Tracking      | 시간에 따라 변하는 target trajectory를 입력이 추적 | SDVX knob, Arcaea arc, osu slider 유사 |
| Repeated      | 시간 구간 내 반복/횟수 기반 입력                   | Taiko roll/balloon                     |
| Composite     | 여러 조건의 조합                                   | 기타도라 fret+strum, arc+tap           |
| Custom        | 위 primitive로 자연스럽지 않은 게임 특화 로직      | 향후 확장                              |

## **8.1 InteractionEvaluator 인터페이스**

pub trait InteractionEvaluator: Send + Sync {  
fn begin(&self, object: &TimedObject, ctx: &BeginContext) -\> Box\<dyn
ActiveInteraction\>;  
}  
  
pub trait ActiveInteraction {  
fn on_input(&mut self, event: &InputEvent, ctx: &InteractionContext) -\>
InteractionOutput;  
fn advance_to(&mut self, song_time: Timestamp, ctx: &InteractionContext)
-\> InteractionOutput;  
fn state(&self) -\> InteractionState;  
}

## **8.2 Tracking 일반화**

target(t) \<-\> actual_input(t)  
\| error metric \|  
v  
tolerance / grace / binding policy

1D scalar(SDVX), 2D touch path(Arcaea), 포인터 경로(osu 유사), 공간
pose(VR)를 같은 “tracking” 개념으로 볼 수 있지만, 입력 타입의 의미
자체를 없애지는 않는다.

# **9. Judge Engine**

## **9.1 파이프라인**

InputEvent  
\|  
v  
Candidate Resolver  
\| 어떤 object가 이 입력의 후보인가?  
v  
Interaction Evaluator  
\| 현재 interaction 조건을 만족하는가?  
v  
Judge Window / Policy  
\| early/late delta, custom rule  
v  
JudgeEvent  
\|  
+--\> Score/Gauge plugin  
+--\> Audio trigger  
+--\> Render effect event

## **9.2 Judge Profile**

pub struct JudgeWindow {  
pub result: JudgeGrade,  
pub early: Duration,  
pub late: Duration,  
}  
  
pub struct JudgeProfile {  
pub windows: Vec\<JudgeWindow\>,  
pub input_offset: Duration,  
pub policy: JudgePolicyId,  
}

early/late를 대칭으로 강제하지 않는다. 게임별 판정 방식과 candidate 선택
규칙도 교체 가능해야 한다.

# **10. 렌더 상태 계산**

런타임은 GPU draw call이나 sprite를 직접 만들지 않는다. “현재 song
time에서 어떤 object가 어떤 논리적 위치/진행 상태를 가져야 하는지”만
계산한다.

pub struct RenderFrame {  
pub song_time: Timestamp,  
pub objects: Vec\<RenderObjectState\>,  
pub transient_events: Vec\<VisualEvent\>,  
}  
  
pub enum RenderObjectState {  
Lane(LaneRenderState),  
Point(PointRenderState),  
Path(PathRenderState),  
Polar(PolarRenderState),  
Custom(CustomRenderState),  
}

## **10.1 게임별 projection 예시**

| **게임 유형**  | **runtime 계산**                            | **renderer 책임**       |
|----------------|---------------------------------------------|-------------------------|
| IIDX/BMS       | lane id, judge line까지 normalized distance | 픽셀 위치, sprite, skin |
| osu!standard형 | 2D object position, approach progress       | circle/slider 렌더      |
| Arcaea형       | path visible range, tracking head/progress  | 3D/2.5D path 표현       |
| maimai형       | angle/radius/progress                       | 원형 UI 실제 그리기     |
| 얼불춤형       | path segment/index/progress                 | 타일/카메라 표현        |
| VR             | target pose/volume/progress                 | 공간 mesh/이펙트        |

## **10.2 성능 규칙**

- 매 프레임 전체 object를 순회하지 않는다.

- 시간순 index/binary search/cursor를 사용해 visible window만 처리한다.

- 렌더 state 생성에서 불필요한 heap allocation을 줄이고 재사용 buffer를
  지원한다.

- 판정용 timeline과 visual scroll speed를 독립시킨다.

# **11. Audio Runtime**

## **11.1 구조**

Gameplay / Judge Thread  
\|  
v  
AudioCommand Queue (lock-free SPSC/MPSC 검토)  
\|  
========== RT boundary ==========  
\|  
v  
Audio Callback  
- scheduled commands consume  
- sample mixing  
- resampling/rate processing  
- output buffer fill  
\|  
v  
Audio Device

## **11.2 AudioCommand**

pub enum AudioCommand {  
Play { sample: SampleId, at: Timestamp, gain: f32 },  
Stop { voice: VoiceId, at: Timestamp },  
SetRate { rate: f64, at: Timestamp },  
Seek { song_time: Timestamp, at: Timestamp },  
}

## **11.3 Realtime 안전 규칙**

| **금지/권장** | **항목**                                            |
|---------------|-----------------------------------------------------|
| 금지          | heap allocation / Vec growth                        |
| 금지          | blocking Mutex/RwLock                               |
| 금지          | disk I/O / network I/O                              |
| 금지          | MP3/OGG 등의 실시간 decode를 callback 내부에서 수행 |
| 금지          | 일반 logging/formatting                             |
| 권장          | preallocated voice/sample structures                |
| 권장          | lock-free queue                                     |
| 권장          | 곡 로딩 시 PCM decode/cache                         |
| 권장          | underrun counter는 atomic/RT-safe telemetry로 기록  |

## **11.4 배속과 역재생**

- pitch 변화 허용 배속: resampling/read-head rate 변경으로 단순 구현
  가능.

- pitch 유지 배속: 별도 time-stretch DSP 모듈로 분리하며 MVP 필수 기능이
  아니다.

- 역재생은 timeline reverse와 sample reverse를 구분한다.

- keysound 기반 게임은 ReverseTimelineOnly / ReverseSamples / Mute
  정책을 선택 가능하게 한다.

# **12. Replay, Seek, Reverse와 상태 복원**

## **12.1 Replay 구성**

ReplayHeader  
- chart_hash  
- rules_hash  
- runtime_version  
- options  
- random_seed  
  
ReplayBody  
- normalized InputEvents  
- optional calibration metadata

동일한 입력 replay를 다시 실행하면 동일 JudgeEvent/score sequence가
나와야 한다.

## **12.2 Snapshot + Event Replay**

Snapshot @ 0s  
Snapshot @ 5s  
Snapshot @ 10s  
...  
  
seek(12.4s):  
restore snapshot @ 10s  
replay deterministic events 10s -\> 12.4s

이 방식은 seek, practice rewind, replay scrub, reverse simulation 기반을
동시에 제공한다. 모든 상태 변화에 undo()를 강제하는 방식보다
유지보수성이 높다.

# **13. 지원 게임 유형 검증 매트릭스**

| **대상**       | **검증할 기능**                      | **주요 primitive**                |
|----------------|--------------------------------------|-----------------------------------|
| IIDX/BMS       | 다중 lane, chord, scratch, keysound  | Instant, Hold, Axis               |
| SDVX           | 버튼/hold + 연속 knob trajectory     | Instant, Hold, Tracking           |
| Taiko          | 양측 discrete hit + roll/balloon     | Instant, Repeated                 |
| GITADORA       | fret 상태 + strum의 복합 판정        | Composite                         |
| Arcaea형       | 멀티터치 ContactId + 2D arc tracking | Tracking, Composite               |
| CHUNITHM형     | 폭/위치/air gesture/센서             | Tracking, Custom/Pose             |
| maimai형       | 원형 터치 + slide/path               | Touch, Tracking                   |
| osu!mania      | lane/hold                            | Instant, Hold                     |
| osu!standard형 | 2D pointer + timed hit + slider      | Pointer, Tracking                 |
| 얼불춤형       | path progression + sequential judge  | Instant + custom candidate policy |
| VR rhythm      | pose, 위치, 속도, 센서 조합          | Pose, Composite, Tracking         |

# **14. 공개 API 및 Composition Root 초안**

`beatkernel` 자체는 플랫폼 backend를 생성하지 않는다. final app이 platform과 game adapter를 조립한다.

```rust
use beatkernel::{BeatKernel, BindingMap};
use beatkernel_platform::DefaultPlatform;

let mut platform = DefaultPlatform::open(Default::default())?;
let game = my_game_adapter::load_chart_and_rules("chart")?;
let bindings = BindingMap::load_or_default()?;

let mut kernel = BeatKernel::builder()
    .chart(game.compiled_chart)
    .judge_profile(game.judge_profile)
    .transport(Default::default())
    .build()?;

while app_running() {
    for physical in platform.input().drain_events()? {
        for game_input in bindings.map(physical) {
            kernel.push_input(game_input)?;
        }
    }

    let output = kernel.advance(platform.host_clock().now())?;
    renderer.render(output.render_frame());
}
```

실제 RT audio wiring은 backend의 callback model에 맞춰 연결하되, `beatkernel`의 mixer/scheduler가 플랫폼 고유 WASAPI/CoreAudio/PipeWire 타입을 직접 알지 않게 한다.

## **14.1 플랫폼 선택**

final app은 `beatkernel-platform` 하나를 의존하고, crate 내부 `cfg(target_os)`가 현재 OS 구현을 선택하게 할 수 있다.

```rust
#[cfg(target_os = "windows")]
pub use windows::Platform as DefaultPlatform;

#[cfg(target_os = "linux")]
pub use linux::Platform as DefaultPlatform;

#[cfg(target_os = "macos")]
pub use macos::Platform as DefaultPlatform;
```

## **14.2 게임 어댑터 원칙**

BMS/IIDX형 구현은 예를 들어 `beatkernel-bms`가 담당한다.

```text
beatkernel-bms -> beatkernel
beatkernel-platform -> beatkernel
beatmania-player -> beatkernel-bms + beatkernel-platform + renderer
```

`beatkernel-bms`가 `beatkernel-platform`을 의존하거나 Windows/Linux/macOS backend를 직접 import하는 구조는 금지한다.

## **14.3 확장 API**

```rust
kernel.register_interaction(my_interaction);
kernel.register_candidate_policy(my_candidate_policy);
kernel.register_visual_projection(my_projection);
platform.input().register_device_adapter(my_arcade_device_adapter);
```

장치 adapter 등록 위치는 backend 구현 세부에 따라 platform registry가 될 수 있지만, adapter가 생성하는 결과 타입은 `beatkernel`의 canonical physical input이어야 한다.

# **15. Threading 모델 초안**

OS Input Thread(s)  
\|  
v  
timestamped event queue  
\|  
v  
Gameplay / Runtime Thread  
- transport  
- object activation  
- judge  
- state  
- render-state production  
\|  
+------\> Renderer Thread / host engine  
\|  
+------\> audio command queue  
\|  
v  
Audio RT Thread

| **초기 원칙** 처음부터 무조건 멀티스레드화하지 않는다. Audio RT thread는 분리하되, judge/runtime core는 우선 단일 소유 thread에서 결정론적으로 구현한다. 성능 측정 후 필요한 부분만 병렬화한다. |
|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

# **16. 성능/품질 목표**

수치는 “보장값”이 아니라 개발 방향을 잡기 위한 초기 목표다. 실제
장치/OS의 물리적 latency와 timestamp 품질을 별도로 기록한다.

| **영역**       | **목표/정책**                                                                     |
|----------------|-----------------------------------------------------------------------------------|
| Judge hot path | 일반 이벤트당 allocation 0, lock 0을 목표                                         |
| Audio callback | allocation 0, blocking lock 0, I/O 0                                              |
| Replay         | 동일 플랫폼/동일 버전에서 bit-level에 가까운 결정성, 가능하면 플랫폼 간 동일 결과 |
| Input          | event loss/order inversion 0, timestamp provenance 기록                           |
| Render state   | visible range 기반 처리, 전체 chart 매 프레임 순회 금지                           |
| Telemetry      | p50/p95/p99/max, jitter, underrun/drop count 제공                                 |
| Startup        | 대형 chart도 compile/index 이후 gameplay path에서 parsing 금지                    |

# **17. 테스트 전략**

## **17.1 Unit Test**

- Timestamp/Duration arithmetic와 overflow 정책
- Transport 1x/0.5x/2x/rate change/pause/seek/reverse mapping
- Windows scan code / Linux evdev / macOS HID sample mapping이 동일 canonical keyboard HID Usage로 수렴하는지 검증
- 같은 canonical key라도 DeviceId가 다르면 독립 입력으로 유지되는지 검증
- BindingMap의 exact-device / any-device selector 동작
- judge window boundary의 정확한 포함/배제
- contact lifecycle 및 rebind 정책
- axis relative/absolute normalization
- Compiled Chart 생성 결과
- snapshot restore 후 동일 event sequence 재생

## **17.2 Property/Fuzz Test**

- 임의 rate change sequence에서도 song time mapping이 anchor 규칙과 일치하는지 검증
- Canonical input event serialization ↔ deserialization round-trip
- native key mapping table에 중복/불가능 mapping이 없는지 검증
- chart parser/compiler fuzzing
- 동일 replay를 N회 실행해 JudgeEvent sequence hash가 동일한지 검증

## **17.3 Integration Test**

- virtual physical keyboard → canonical key → BindingMap → 4-key chart → judge → audio command end-to-end
- 두 개의 virtual keyboard에서 동일 physical key를 눌러도 device-specific binding이 구분되는지 검증
- raw HID fixture → DeviceAdapter → canonical button/axis event 검증
- virtual axis stream → tracking curve 판정
- 멀티터치 두 ContactId → 독립 arc tracking
- seek/reverse 후 snapshot restore consistency
- audio scheduler command ordering과 sample offset 테스트

## **17.4 Hardware Benchmark**

- Windows Raw Input/GameInput/HID backend별 이벤트 timestamp와 jitter 비교
- Linux evdev/hidraw, macOS IOHID에서 가능한 timestamp provenance 기록
- 입력 1,000Hz 이상의 synthetic/source 장치에서 event loss/order 검사
- audio buffer size별 underrun 및 scheduling jitter
- 가능하다면 loopback 장비를 사용한 input-to-audio end-to-end latency 측정

# **18. 개발 단계 및 완료 조건**

| **단계** | **구현 내용** | **완료 조건** |
|---|---|---|
| Phase 0 — Repository Skeleton | `beatkernel` + `beatkernel-platform` 2-crate workspace, CI, fmt/clippy/test, target-specific dependency 틀을 만든다. | 두 crate가 최소 API/문서와 함께 빌드되고 비대상 OS dependency가 현재 target에서 요구되지 않는다. |
| Phase 1 — Time + Transport | `beatkernel` 내부 module로 정수 timestamp, clock domain, rate/pause/seek/reverse를 구현한다. | rate 변경 1,000회 property test, reverse/seek 테스트 통과. |
| Phase 2 — Canonical Physical Input | DeviceId, DeviceDescriptor, HID Usage 기반 PhysicalControlId, Button/Axis/Touch/Pointer/Pose, virtual backend를 구현한다. | 동일 물리 키의 OS별 fixture가 동일 canonical ID로 수렴하고 multi-device identity가 보존된다. |
| Phase 3 — Binding Layer | `PhysicalControlId + DeviceSelector -> GameControlId` mapping과 passthrough touch/pose channel을 구현한다. | 키보드 2대의 같은 키를 서로 다른 game control로 매핑하는 테스트 통과. |
| Phase 4 — Windows Native Input MVP | Raw Input + generic HID/raw report path와 input-inspector를 구현한다. | 실제 키 입력에서 device id, native code, canonical HID usage, native/normalized timestamp를 확인할 수 있다. |
| Phase 5 — Chart Compiler | simple source chart를 absolute `CompiledChart`로 compile한다. | BPM change/STOP synthetic golden test 통과. |
| Phase 6 — Judge MVP | 4-key Instant/Hold, candidate resolver, configurable early/late windows를 구현한다. | canonical/bound input으로 콘솔 4-key 플레이와 deterministic result 재현. |
| Phase 7 — Audio MVP | PCM preload, RT scheduler/mixer, platform audio trait와 Windows output을 구현한다. | callback allocation/blocking lock이 없고 scheduled hit가 sample offset 단위로 재현된다. |
| Phase 8 — Integrated Low-Latency Loop | Windows physical input → binding → judge → audio를 연결한다. | p50/p95/p99 timing telemetry, event drop/underrun counter를 확인 가능. |
| Phase 9 — Visual Projection | Lane/Point/Path 최소 projection과 RenderFrame API를 구현한다. | 외부 demo renderer가 runtime state만으로 4-key/path fixture를 그린다. |
| Phase 10 — Replay + Snapshot | normalized game/physical input replay, snapshot, seek/reverse restore를 구현한다. | 임의 시점 seek 후 기준 직선 재생과 state hash 일치. |
| Phase 11 — Generalization Fixtures | SDVX-like, Arcaea-like, Taiko/GITADORA-like synthetic game adapter를 추가한다. | `beatkernel`에 게임명 분기를 추가하지 않고 서로 다른 입력 모델을 구현한다. |
| Phase 12 — Linux Backend | evdev + hidraw input, 선택한 저지연 audio backend를 추가한다. | 같은 표준 키보드 fixture가 Windows와 동일 canonical key/binding 결과를 낸다. |
| Phase 13 — macOS Backend | IOHID 계열 input + CoreAudio output을 추가한다. | 같은 canonical mapping/replay/judge suite 통과. |
| Phase 14 — Real Game Adapter | `beatkernel-bms` 같은 실제 parser/rules adapter를 별도 crate로 추가한다. | adapter가 `beatkernel-platform` 없이 테스트되고 final sample app에서 platform과 조립된다. |
| Phase 15 — FFI/SDK | 필요성이 확인되면 C ABI/host SDK를 최소 제공한다. | C/C# sample에서 create/input/update/destroy 또는 동등 흐름이 가능하다. |

# **19. AI 코딩 에이전트 작업 규칙**

1. 한 Phase를 완료하기 전에 다음 Phase의 대규모 기능을 선행 구현하지 않는다.
2. `time`, `transport`, `input`, `chart`, `judge`, `audio`, `runtime`을 이유 없이 별도 crate로 분리하지 않는다. 기본 위치는 `beatkernel` 내부 module이다.
3. 새 crate는 플랫폼/FFI 격리, 독립 배포, 실질적 의존성 절감 같은 구체 이유가 있을 때만 제안한다.
4. `beatkernel-platform`은 `beatkernel`을 의존할 수 있지만, `beatkernel`은 platform crate를 의존하지 않는다.
5. 게임 어댑터는 `beatkernel`을 의존할 수 있지만 `beatkernel-platform`을 의존하지 않는다. 둘의 조립은 final app에서만 한다.
6. Windows scan code, Linux evdev code, macOS native key code를 game rule에 직접 노출하지 않는다. 가능한 경우 canonical HID Usage로 정규화한다.
7. 키 binding은 문자열 문자값이 아니라 physical identity를 기준으로 한다. logical/text input은 gameplay binding과 분리한다.
8. 새 추상화가 필요하면 먼저 해결하려는 두 개 이상의 구체 사례와 테스트를 제시한다.
9. 게임명을 `beatkernel` 조건문에 넣지 않는다. 게임별 규칙은 adapter/profile/test fixture에 둔다.
10. Audio callback에 allocation/blocking lock/I/O가 생기는 변경은 거부한다.
11. Timestamp를 f64 seconds로 바꾸는 변경은 거부한다. 변환은 경계에서만 한다.
12. 모든 public API 변경에는 사용 예와 테스트를 추가한다.
13. 성능 최적화는 benchmark로 전후 차이를 확인한다. 추측으로 unsafe/lock-free를 남발하지 않는다.
14. unsafe는 OS FFI 또는 명확히 측정된 필요 지점에 국한하고 SAFETY 주석과 테스트를 남긴다.
15. 새 장치는 core type 추가보다 기존 canonical physical event + DeviceAdapter로 표현 가능한지 먼저 검토한다.
16. 새 게임은 core primitive 추가보다 Custom evaluator/game adapter로 먼저 구현하고 반복 패턴이 확인되면 primitive 승격을 검토한다.

# **20. 구현 시 피해야 할 안티패턴**

| **안티패턴** | **왜 문제인가** | **대안** |
|---|---|---|
| time/judge/ir/runtime를 전부 별도 crate로 분리 | API 경계/의존성 관리만 증가하고 함께 진화하기 어려움 | `beatkernel` 내부 module 우선 |
| game adapter가 windows/linux/macos backend를 직접 의존 | 게임 로직과 플랫폼이 결합됨 | final app composition root에서 조립 |
| 게임에서 Windows scan code/evdev code 사용 | 설정과 replay가 OS에 종속됨 | canonical PhysicalControlId + Binding |
| 문자 `A`/`ㅁ`를 gameplay key identity로 사용 | 키보드 레이아웃에 따라 binding 의미가 변함 | USB HID Usage 기반 physical key |
| 모든 입력을 `Vec<f32>`로 통일 | 장치 의미와 lifecycle 소실 | typed physical event + Custom escape hatch |
| Note enum에 게임별 타입 무한 추가 | core가 게임 카탈로그가 됨 | TimedObject + Interaction/game adapter |
| game loop에서 poll 후 `Instant::now()` | 입력 timestamp가 frame timing에 오염 | native timestamp 획득/clock mapping |
| `current_time = elapsed * current_rate` | 과거 구간까지 새 rate가 적용되어 점프 | Transport anchor piecewise mapping |
| 판정과 note 위치 계산 결합 | SV/배속/역스크롤이 판정에 영향 | Judge/Visual timeline 분리 |
| audio callback에서 decode/mutex | glitch/underrun/jitter | predecode + RT-safe queue |
| 모든 상태 변화에 undo 구현 | 복잡성과 버그 급증 | snapshot + deterministic replay |
| 첫날부터 판정 DSL/VM 구현 | 요구사항을 모른 채 언어부터 설계 | Rust trait로 여러 fixture 검증 후 일반화 |

# **21. MVP 범위 — 실제 첫 개발 목표**

| **MVP 정의** Windows에서 4-key synthetic chart를 로드하고, Raw Input 키보드 이벤트를 `DeviceId + canonical HID Usage + normalized timestamp`로 변환한 뒤 사용자 Binding을 거쳐 configurable judge를 수행하고, hit에 연결된 predecoded PCM을 낮은 latency로 출력한다. console telemetry와 deterministic replay를 함께 제공한다. |
|---|

- 실제 crate는 우선 `beatkernel` + `beatkernel-platform` 두 개
- Windows Raw Input keyboard backend
- generic raw HID report path와 DeviceAdapter API
- canonical keyboard HID Usage mapping
- 여러 키보드의 DeviceId 구분
- PhysicalControl → GameControl BindingMap
- 4-key Instant + Hold
- BPM change/STOP synthetic chart compiler
- configurable early/late judge window
- PCM WAV preload + RT scheduler/mixer
- input/judge/audio timing telemetry
- replay record/playback
- 렌더링은 콘솔 또는 최소 debug visualization

이 MVP가 안정적으로 동작한 뒤 Axis tracking(SDVX형), Contact tracking(Arcaea형), Composite(GITADORA형)를 순서대로 추가한다. Linux/macOS는 canonical input API가 Windows에서 충분히 검증된 뒤 같은 public model에 backend만 추가한다.

# **22. 2차 범용성 검증 순서**

| **순서** | **fixture**          | **검증 목적**                                  |
|----------|----------------------|------------------------------------------------|
| 1        | SDVX-like laser      | Axis absolute/relative, Tracking               |
| 2        | Arcaea-like dual arc | ContactId, multi-touch, 2D Tracking            |
| 3        | Taiko-like roll      | Repeated interaction                           |
| 4        | GITADORA-like chord  | 여러 입력 상태 + trigger Composite             |
| 5        | osu!-like pointer    | Pointer trajectory + instant hit               |
| 6        | VR sample            | Pose input + derived/game-specific interaction |

# **23. 초기 파일/모듈 단위 TODO**

Phase 0~1 권장 구조:

```text
crates/beatkernel/src/
├─ lib.rs
├─ time/
│  ├─ mod.rs
│  ├─ timestamp.rs
│  ├─ duration.rs
│  └─ clock_domain.rs
└─ transport/
   ├─ mod.rs
   ├─ transport.rs
   ├─ anchor.rs
   └─ rate.rs

crates/beatkernel-platform/src/
├─ lib.rs
├─ windows/mod.rs
├─ linux/mod.rs
└─ macos/mod.rs
```

Phase 2~3에서 `beatkernel::input`을 확장한다.

```text
crates/beatkernel/src/input/
├─ mod.rs
├─ device.rs
├─ control.rs
├─ key.rs
├─ event.rs
├─ touch.rs
├─ pose.rs
└─ binding.rs
```

Phase 4 Windows backend:

```text
crates/beatkernel-platform/src/windows/
├─ mod.rs
├─ input.rs
├─ raw_input.rs
├─ hid.rs
├─ key_map.rs
├─ device_registry.rs
├─ clock.rs
└─ audio.rs        # Audio phase에서 구현
```

`key_map.rs`는 native scan code/HID 정보에서 canonical HID Usage로의 변환을 책임지며, 게임별 control 이름을 포함하지 않는다.

이후 chart/judge/audio/replay는 별도 crate가 아니라 우선 `crates/beatkernel/src/...` module로 추가한다.

# **24. Definition of Done**

프로젝트의 “코어 완성”을 단순히 컴파일되는 상태로 보지 않는다. 아래 조건을 충족해야 1.0 후보로 본다.

- `beatkernel`이 OS native API에 직접 의존하지 않는다.
- `beatkernel-platform`과 게임 어댑터가 서로 의존하지 않고 final app에서만 만난다.
- Windows/macOS/Linux 중 최소 Windows + 한 플랫폼에서 native input/audio backend가 동작한다.
- 표준 키보드의 동일 물리 키가 지원 OS에서 동일 canonical HID Usage ID로 정규화된다.
- 동일 canonical key라도 서로 다른 DeviceId를 가진 입력을 독립적으로 binding할 수 있다.
- 커스텀 raw HID report를 DeviceAdapter로 canonical physical event로 변환할 수 있다.
- Instant/Hold/Tracking/Repeated/Composite를 실제 fixture로 검증했다.
- Button/Axis/Touch Contact/Pointer/Pose를 공통 runtime에 전달할 수 있다.
- rate/pause/seek/reverse가 Transport 테스트를 통과한다.
- 판정과 visual projection이 분리되어 있다.
- audio RT path의 금지 항목이 audit/benchmark로 확인된다.
- replay를 반복 실행했을 때 동일 판정 결과를 낸다.
- snapshot 기반 seek 후 state hash가 기준 실행과 일치한다.
- `beatkernel`에 특정 게임명/OS명 기반 분기 없이 최소 4종의 서로 다른 게임 fixture를 구현했다.
- latency/jitter/underrun telemetry와 benchmark binary가 있다.
- public API 예제와 crate-level documentation이 준비되어 있다.

# **25. AI에게 바로 줄 시작 프롬프트**

아래 요구사항을 AI 에이전트의 첫 구현 작업으로 사용할 수 있다.

```text
You are implementing Phase 0 and Phase 1 of BeatKernel as described in this document.

Goals:
1. Create a Rust workspace with exactly two initial crates:
   - crates/beatkernel
   - crates/beatkernel-platform
2. Keep time, transport, input, chart, judge, visual, audio, replay, and runtime as modules inside beatkernel unless this document explicitly promotes one to a crate.
3. Implement integer Timestamp/Duration and ClockDomain identifiers in beatkernel::time.
4. Implement anchor-based piecewise host-time -> song-time mapping in beatkernel::transport.
5. Support rate > 0, rate = 0, rate < 0, pause, seek, and runtime rate changes.
6. Prepare beatkernel-platform with cfg-gated windows/linux/macos modules and target-specific dependency sections, but do not implement native I/O yet.
7. Add comprehensive unit/property tests for continuity, reverse, pause, seek, and repeated rate changes.

Architecture constraints:
- beatkernel MUST NOT depend on beatkernel-platform.
- Game adapters MUST NOT depend on beatkernel-platform.
- Platform and game adapters are composed only by a final app/example.
- Do not create beatkernel-time, beatkernel-ir, beatkernel-judge, beatkernel-runtime, or similar micro-crates.
- Do not use f64 as the canonical timestamp representation.
- Do not add game names, lane-specific assumptions, Windows scan codes, evdev codes, or macOS key codes to core gameplay APIs.
- Keep unsafe out of Phase 0/1.
- Public types need useful rustdoc and tests.
- cargo fmt, cargo clippy --workspace --all-targets --all-features, and cargo test --workspace must pass.

Before coding:
A. Show the exact workspace tree.
B. Show the dependency graph and confirm it is acyclic.
C. Summarize the public API you will implement in Phase 1.

Then implement only Phase 0 and Phase 1. Do not broaden scope.
```

# **26. 최종 설계 요약**

BeatKernel의 핵심은 “모든 리듬게임을 하나의 거대한 Note enum으로 우겨 넣는 것”도, “모든 논리 단위를 별도 crate로 쪼개는 것”도 아니다.

장치와 OS의 입력은 가장 아래에서 native 의미와 timestamp를 최대한 보존한다. 표준 키보드처럼 공통 identity가 있는 입력은 HID Usage 기반 canonical physical control로 정규화하고, 같은 키라도 `DeviceId`로 장치를 구분한다. 커스텀 HID는 DeviceAdapter가 raw report를 canonical event로 변환한다.

그 위의 Binding 계층은 물리 입력을 게임 control/channel로 바꾼다. 따라서 BMS/IIDX형 게임 어댑터는 `KEY1`, `SCRATCH` 같은 의미만 알고 Windows scan code, Linux evdev, macOS HID backend는 전혀 알지 않는다.

`beatkernel`은 공통 시간축, Transport, Chart/Compiled Chart, Interaction/Judge, Visual State, Audio Scheduler, Replay를 한 crate 안의 명확한 module로 제공한다. `beatkernel-platform`은 native input/audio/clock을 구현하고, 실제 게임 어댑터는 `beatkernel` 위에 올라간다. 최종 실행 app만 플랫폼과 게임을 조립한다.

```text
Device meaning stays meaningful.
Physical keys become canonical where possible.
Device identity is never discarded.
Platform and game dependencies never cross.
Timing becomes common.
Game rules remain adapters/policies.
Audio stays real-time safe.
Rendering receives state, not draw calls.
Replay makes the runtime testable and reversible.
Modules are cheap; crates are created only when justified.
```

| **최종 원칙** 추상화의 목적은 차이를 지우는 것이 아니라, 공통화할 수 있는 identity/time/ordering을 정규화하면서 장치·게임 고유 의미는 필요한 층에 그대로 남기는 것이다. |
|---|
