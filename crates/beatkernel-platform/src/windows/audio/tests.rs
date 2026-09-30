use super::*;
use std::sync::atomic::AtomicBool;

fn request_for_unit_stream() -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("owned-thread-unit-seam".into()),
        AudioBackendKind::Wasapi,
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3)).unwrap(),
        BufferRequest::Frames(480),
        PeriodRequest::Frames(240),
    )
    .unwrap()
}

#[test]
fn native_buffer_validation_preserves_reported_engine_and_wake_specific_constraints() {
    for event_driven in [true, false] {
        let selected = request_for_unit_stream();
        let reported = PeriodConstraints {
            default_frames: Some(240),
            default_period: Some(Duration::from_nanos(5_000_000)),
            min_frames: Some(96),
            max_frames: Some(480),
            fundamental_frames: Some(48),
            alignment_frames: Some(128),
            min_buffer_duration: Some(Duration::from_nanos(10_000_000)),
            max_buffer_duration: Some(Duration::from_nanos(20_000_000)),
            buffer_bounds_event_driven: Some(event_driven),
            ..PeriodConstraints::default()
        };
        assert_eq!(
            validate_native_buffer_size(&selected, 512, reported),
            Err(AudioPlatformError::ConfigurationUnsupported {
                constraint: ConfigurationConstraint::EngineManagedBuffer,
                constraints: reported,
                suggested_buffer_frames: Some(512),
                suggested_period_frames: None,
            })
        );
        assert_eq!(
            validate_native_buffer_size(&selected, 480, reported),
            Ok(false)
        );
    }
}

struct WorkerDropFlag(Arc<AtomicBool>);
impl Drop for WorkerDropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn failing_ack_stream(panic_after_ack: bool) -> (WasapiStream, Arc<AtomicBool>) {
    let request = request_for_unit_stream();
    let format = request.format();
    let configuration = AppliedStreamConfig {
        requested: request,
        format,
        buffer_frames: 480,
        buffer_duration: Duration::from_nanos(10_000_000),
        period_frames: Some(240),
        period_duration: Duration::from_nanos(5_000_000),
        stream_latency: Duration::ZERO,
        sizing_adjusted: false,
    };
    let control = Arc::new(Control {
        request: AtomicU8::new(0),
        telemetry: Telemetry::new(),
        cadence: crate::audio::cadence::Capture::new(),
    });
    let dropped = Arc::new(AtomicBool::new(false));
    let (ack_tx, started) = mpsc::sync_channel(1);
    let wake = Event::new().unwrap();
    let worker = {
        let control = Arc::clone(&control);
        let dropped = Arc::clone(&dropped);
        thread::spawn(move || {
            let _drop_flag = WorkerDropFlag(dropped);
            control.telemetry.status.store(
                status_code(AudioStreamStatus::Failed {
                    hresult: E_FAIL_CODE,
                }),
                Ordering::SeqCst,
            );
            ack_tx.send(Err(E_FAIL_CODE)).unwrap();
            if panic_after_ack {
                panic!("deliberate owned-worker panic after failed acknowledgment");
            }
            let deadline = Instant::now() + StdDuration::from_secs(5);
            while control.request.load(Ordering::Acquire) != 2 {
                assert!(
                    Instant::now() < deadline,
                    "Start failure did not request worker teardown"
                );
                thread::yield_now();
            }
        })
    };
    (
        WasapiStream {
            configuration,
            options: WasapiOptions::default(),
            control,
            wake,
            worker: Some(worker),
            started,
            has_started: false,
        },
        dropped,
    )
}

#[test]
fn failed_start_acknowledgment_joins_owned_thread_before_returning_original_error() {
    // This uses a real Event and owned thread to hit production start/stop/join;
    // it does not execute or attest a failing native COM Start call.
    let (mut stream, dropped) = failing_ack_stream(false);
    assert_eq!(
        stream.start(),
        Err(AudioPlatformError::Native { code: E_FAIL_CODE })
    );
    assert!(dropped.load(Ordering::Acquire));
    assert!(stream.worker.is_none());
    assert_eq!(stream.control.request.load(Ordering::Acquire), 2);
    assert_eq!(
        stream.snapshot().status,
        AudioStreamStatus::Failed {
            hresult: E_FAIL_CODE
        }
    );
    assert_eq!(stream.start(), Err(AudioPlatformError::WorkerFailure));
}

#[test]
fn failed_start_acknowledgment_preserves_joined_worker_panic_reporting() {
    let (mut stream, dropped) = failing_ack_stream(true);
    assert_eq!(stream.start(), Err(AudioPlatformError::WorkerFailure));
    assert!(dropped.load(Ordering::Acquire));
    assert!(stream.worker.is_none());
    assert_eq!(stream.snapshot().status, AudioStreamStatus::WorkerPanicked);
}

fn decode(wave: &WAVEFORMATEXTENSIBLE) -> Result<DeviceFormat, AudioPlatformError> {
    // SAFETY: This owned full stack structure physically contains WAVEFORMATEX
    // plus its complete 22-byte extension; all fixtures declare cbSize<=22.
    unsafe { read_format(std::ptr::addr_of!(wave.Format)) }
}

#[test]
fn production_waveformat_packing_and_readback_preserve_pcm_widths_and_direct_output() {
    for (container_bits, valid_bits, channels, mask) in [
        (16, 16, 1, None),
        (24, 24, 2, None),
        (32, 32, 2, Some(3)),
        (24, 20, 2, Some(0)),
        (32, 24, 3, Some(7)),
        (16, 1, 1, Some(4)),
    ] {
        let format = DeviceFormat::new(
            12_345,
            channels,
            SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            },
            mask,
        )
        .unwrap();
        let wave = wave_format(format);
        let base = wave.Format;
        assert_eq!(
            (
                base.nSamplesPerSec,
                base.nChannels,
                base.nBlockAlign,
                base.nAvgBytesPerSec,
                base.wBitsPerSample
            ),
            (
                12_345,
                channels,
                channels * (container_bits / 8),
                12_345 * u32::from(channels) * (u32::from(container_bits) / 8),
                container_bits
            )
        );
        let extensible = mask.is_some() || container_bits != valid_bits;
        assert_eq!(
            (base.wFormatTag, base.cbSize),
            (
                if extensible { 0xfffe } else { 1 },
                if extensible { 22 } else { 0 }
            )
        );
        let subformat = wave.SubFormat;
        assert_eq!(subformat, PCM_GUID);
        // SAFETY: PCM's documented active union member is wValidBitsPerSample.
        let actual_valid = unsafe { wave.Samples.wValidBitsPerSample };
        assert_eq!(actual_valid, valid_bits);
        let actual_mask = wave.dwChannelMask;
        assert_eq!(actual_mask, mask.unwrap_or(0));
        assert_eq!(decode(&wave), Ok(format));
    }
}

#[test]
fn production_waveformat_float_guid_and_unspecified_layout_roundtrip() {
    for mask in [None, Some(0), Some(3)] {
        let format = DeviceFormat::new(48_000, 2, SampleEncoding::Float32, mask).unwrap();
        let wave = wave_format(format);
        let base = wave.Format;
        let subformat = wave.SubFormat;
        assert_eq!(
            (base.wFormatTag, base.cbSize),
            (
                if mask.is_some() { 0xfffe } else { 3 },
                if mask.is_some() { 22 } else { 0 }
            )
        );
        assert_eq!(subformat, FLOAT_GUID);
        assert_eq!(decode(&wave), Ok(format));
    }
}

#[test]
fn native_owned_format_readback_rejects_reserved_speaker_assignments_with_matching_counts() {
    for (channels, valid_mask, reserved_mask) in [
        (1, 4, 0x0004_0000),
        (1, 4, 0x8000_0000),
        (2, 3, 0x8000_0001),
    ] {
        let format =
            DeviceFormat::new(48_000, channels, SampleEncoding::Float32, Some(valid_mask)).unwrap();
        let mut wave = wave_format(format);
        assert_eq!(decode(&wave), Ok(format));
        wave.dwChannelMask = reserved_mask;
        // decode uses the complete owned stack allocation and production FFI
        // parser. The mutation preserves channel popcount, isolating bit validity.
        assert_eq!(decode(&wave), Err(AudioPlatformError::InvalidFormat));
    }
}

#[test]
fn native_format_readback_rejects_invalid_full_owned_structures_before_use() {
    let valid =
        wave_format(DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3)).unwrap());
    for change in 0..10 {
        let mut wave = valid;
        match change {
            0 => wave.Format.nChannels = 0,
            1 => wave.Format.nSamplesPerSec = 0,
            2 => wave.Format.nBlockAlign = 1,
            3 => wave.Format.nAvgBytesPerSec = 1,
            4 => wave.Format.cbSize = 0,
            5 => wave.SubFormat = PCM_GUID,
            6 => {
                wave.Samples = WAVEFORMATEXTENSIBLE_0 {
                    wValidBitsPerSample: 31,
                }
            }
            7 => wave.dwChannelMask = 1,
            8 => wave.Format.wBitsPerSample = 16,
            _ => wave.SubFormat = GUID::from_u128(0),
        }
        // A PCM subformat is valid with 32 bits, unlike the other mutations.
        if change == 5 {
            assert_eq!(
                decode(&wave).unwrap().encoding(),
                SampleEncoding::Pcm {
                    container_bits: 32,
                    valid_bits: 32
                }
            );
        } else {
            assert_eq!(decode(&wave), Err(AudioPlatformError::InvalidFormat));
        }
    }
    // SAFETY: Null is explicitly rejected without dereferencing any pointer.
    assert_eq!(
        unsafe { read_format(std::ptr::null()) },
        Err(AudioPlatformError::InvalidFormat)
    );
}
