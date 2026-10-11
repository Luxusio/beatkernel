# 대량 노트 표시 용량

동시에 보이는 노트와 지뢰의 2,048개 제한을 제거하고 기존 시간 인덱스로
전체 가시 객체를 조회하도록 변경했다. Scene은 표시 후보의 실패를 유지하며,
Renderer는 모든 필드의 인스턴스 수와 실제 장치 버퍼 한도를 검사한 뒤
제출한다. GPU 버퍼는 작은 크기로 시작하고 필요할 때만 기하급수적으로
확장하며 소스 승인 한도와 장치 한도를 넘지 않는다. browser wire 형식은
그대로 유지한다. 메모리 추정에는 이전 채보에서 남은 scratch, cache,
geometry와 버퍼 교체 중의 저장 공간을 포함한다. 로컬 기능 검증을 마쳤으며,
독립 리뷰와 QA는 별도 완료 조건이다.

범위별 unit 테스트 121개와 별도의 wire 통합 테스트 11개가 통과했다.
공개 playfield 함수, 로컬 roster/page/reservation 변환과 브라우저 상태
변환에서 발생한 오류는 공통 candidate 경계에서 첫 오류로 유지된다.
정상 필드 세 개가 이미 있어도 실패 이후에는 제출할 수 없으며,
`clear()` 후 완전한 후보를 다시 구성한다. 브라우저의 미준비 scalar가
나중에 준비돼도 실패한 Scene을 자동으로 되살리지 않는다.
추가로 독립 CLI QA가 native fixture
`app/tests/dense_native_gpu.rs`를 Xvfb와 Vulkan/llvmpipe에서 실행해
4608개 노트와 2304개 지뢰의 대량 표시, warmed redraw, seek, resize,
잘못된 네 번째 필드 후보의 거절 및 clear 복구를 확인했다. 성공적인
presentation은 12회였다. 인덱스 오류, 직접 foreign progress와 로컬
roster 거절 시 counter는 각각 9, 10, 11에서 증가하지 않았다.
11,520개 인스턴스의 정확한 구성은 내부 CPU
fixture의 별도 oracle로 검사한다. native fixture의 제출 횟수만으로
모든 픽셀이나 인스턴스별 GPU 결과를 검사했다고 주장하지 않는다.

browser fixture `app/web/dense-playfield.browser.mjs`는 실제 BMS와 WAV를
선택하고 production Worker와 RenderWorker의 호출·응답을 관찰한다.
독립 browser QA가 새 WASM SHA256
`8615f7b360972cc1b61eb65678a5e6c62925ef19233e4fa759668a7232a50142`에서
cold 11,520개 draw, warmed 업로드 재사용, 0.25초 seek의 10,943개 draw,
640×480 canvas resize와 resize 후 재사용의 다섯 단계를 통과했다.
브라우저 viewport는 1200×1000이며 좁은 페이지 반응형 검증은 아니다.
wire 패킷,
실제 GPU 업로드와 draw, 양의 geometry ACK를 서로 구별해 검사한다.
기존 150ms trailing window를 포함하는 독립 선형 membership oracle을
사용한다. 제품 오류는 없었으며 브라우저·Xvfb·server와 임시 저장 공간을
정리했다. 원본과 다섯 스크린샷은 ignored 경로
`target/wf/dense-playfield-presentation-budget/qa-browser-final-1/runtime/`에 있다.
native와 CLI 원본은 같은 상위 경로의 `qa-cli-final-1/`에 있다.
이 경로들은 ignored 로컬 증거이며 배포 파일에 포함되지 않는다.

## Known ceiling

- software GPU 검증은 물리 장치의 성능과 입력·오디오 지연을 증명하지 않는다.
- global allocator, Arc 생성과 GPU 드라이버의 모든 OOM을 복구하지는 못한다.
- 공개 notes를 같은 길이로 임의 수정하면 기존 prepared index와 어긋날 수
  있으므로 기존 준비 경로에서 다시 준비해야 한다.
- 메모리 추정은 요청한 payload/layout 저장 공간을 대상으로 한다. allocator와
  driver overhead, surface, 정적 UI cache와 비시각 상태를 포함한 resident
  RAM 상한을 보장하지 않는다. 교체가 겹치는 이전 cold registration의
  저장 공간은 따로 합산해야 한다.

요구사항과 검증 기준은
[고밀도 채보의 완전한 표시](../ui/REQ__dense-playfield-presentation.md)를 따른다.
