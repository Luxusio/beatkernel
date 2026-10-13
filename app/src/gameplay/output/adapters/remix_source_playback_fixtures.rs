//! Static remix forwarding keeps non-Copy native evidence and exact packets intact.
use super::*;
use crate::gameplay::output::{
    application::owner::source_playback_owner_fixtures as memory,
    ports::SourcePlaybackNativeOutputBackend,
};

#[test]
fn remix_forwards_coherent_observation_without_copy_clone_or_separate_observer() {
    let (backend, output, io, _producer) = memory::remix_fixture();
    let basis = output.basis;
    let mut remixed = RemixedOutputBackend::new(backend);
    let mut output = output;
    let observation = memory::observation(basis, 2, 100, output.report);
    let pointer = observation
        .source()
        .native_evidence()
        .unwrap()
        .words
        .as_ptr();
    io.observations
        .borrow_mut()
        .push_back(Ok(Some(observation)));
    let got = remixed
        .observe_native_playback(&mut output, memory::identity())
        .unwrap()
        .unwrap();
    assert_eq!(
        got.source().native_evidence().unwrap().words.as_ptr(),
        pointer
    );
    assert_eq!(got.source().native_evidence().unwrap().words, [19, 31]);
    assert_eq!(got.source().facts().render, output.report);
    assert_eq!(io.observed_calls.get(), 1);
    assert_eq!(io.report_calls.get(), 0);
    assert_eq!(io.legacy_calls.get(), 0);
}
#[test]
fn remix_forwards_readonly_exact_query_and_missing_packet_verbatim() {
    let (backend, output, io, _producer) = memory::remix_fixture();
    let basis = output.basis;
    let remixed = RemixedOutputBackend::new(backend);
    let q = memory::query(3, basis.sample_rate());
    let packet = memory::packet(basis, 7, memory::identity(), q, output.report);
    let pointer = packet.native_evidence().unwrap().words.as_ptr();
    io.queries.borrow_mut().push_back(Ok(Some(packet)));
    let got = remixed
        .source_playback_at(&output, &q, memory::identity())
        .unwrap()
        .unwrap();
    assert_eq!(
        got.query().compare_interpolated_time(q),
        std::cmp::Ordering::Equal
    );
    assert_eq!(got.native_evidence().unwrap().words.as_ptr(), pointer);
    assert!(remixed
        .source_playback_at(&output, &q, memory::identity())
        .unwrap()
        .is_none());
    assert_eq!(io.query_calls.get(), 2);
    assert_eq!(io.report_calls.get(), 0);
}
