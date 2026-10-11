# 대량 노트 표시 용량

동시에 보이는 노트와 지뢰의 2,048개 제한을 제거하고 기존 시간 인덱스로
전체 가시 객체를 조회하도록 변경했다. Scene은 표시 후보의 실패를 유지하며,
Renderer는 모든 필드의 인스턴스 수와 실제 장치 버퍼 한도를 검사한 뒤
제출한다. GPU 버퍼는 작은 크기로 시작하고 필요할 때만 기하급수적으로
확장하며 소스 승인 한도와 장치 한도를 넘지 않는다. browser wire 형식은
그대로 유지한다. 메모리 추정에는 이전 채보에서 남은 scratch, cache,
geometry와 버퍼 교체 중의 저장 공간을 포함한다. 로컬 기능 검증을 마쳤으며,
독립 리뷰와 QA는 별도 완료 조건이다.

범위별 CPU·wire 테스트 117개가 통과했다. 추가로 native fixture
`app/tests/dense_native_gpu.rs`를 Xvfb와 Vulkan/llvmpipe에서 실행해
4608개 노트와 2304개 지뢰의 대량 표시, warmed redraw, seek, resize,
잘못된 네 번째 필드 후보의 거절 및 clear 복구를 확인했다. 성공적인
presentation은 10회였다. 11,520개 인스턴스의 정확한 구성은 내부 CPU
fixture의 별도 oracle로 검사한다. native fixture의 제출 횟수만으로
모든 픽셀이나 인스턴스별 GPU 결과를 검사했다고 주장하지 않는다.

browser fixture `app/web/dense-playfield.browser.mjs`는 실제 BMS와 WAV를
선택하고 production Worker와 RenderWorker의 호출·응답을 관찰한다.
새 WASM SHA256 `17c9301728bbb618a175250d4d725fc3abc2ac556e391ef2d79885e635bf3caa`에서
cold 11,520개 draw, warmed 업로드 재사용, 0.25초 seek의 10,943개 draw,
640×480 resize와 resize 후 재사용의 다섯 단계를 통과했다. wire 패킷,
실제 GPU 업로드와 draw, 양의 geometry ACK를 서로 구별해 검사한다.
기존 150ms trailing window를 포함하는 독립 선형 membership oracle을
사용한다. 제품 오류는 없었으며 브라우저·Xvfb·server와 임시 저장 공간을
정리했다. 원본과 다섯 스크린샷은 ignored 경로
`target/wf/dense-playfield-presentation-budget/browser-smoke-4/`에 있다.

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
