# Windows Raw Input acquisition

Phase 4 adds Windows keyboard and generic HID acquisition to the existing
canonical input model. The OS-independent kernel stays unchanged. The durable
behavior contract is [Windows input](../kernel/REQ__windows-input.md).

The portable parser checks Win32/Win64 layouts, retains complete native fields,
and rejects malformed or oversized packets. The source registry assigns fresh
runtime IDs after reconnect and tracks held keys, repeats and modeled Pause
assembly per device. Opaque HID reports retain wire bytes and batch order;
`report_id: None` means no separate ID was decoded. Acquisitions share one
checked sequence and clock provenance across report fanout.

The native backend reads DWORD-aligned buffers, bounds device/name queries and
retries, and samples a caller-shared QPC origin before reading input. It retains
optional original `MSG.time` separately. Applications explicitly register their
own window, serialize registration changes and grant exclusive class ownership
through guard cleanup. Conflicting registrations are refused; cleanup preserves
detectably changed registrations. Actual Windows testing found that registration
queries omit `RIDEV_DEVNOTIFY`, so cleanup compares the observable bits. The
4,096-class cap applies after deduplication, including duplicate-heavy requests.

The console inspector owns a finite message pump and window. Foreground input
reaches `DefWindowProc` exactly once before acquisition errors propagate.
Synchronous close posts quit until registration cleanup finishes. Help, bounded
duration, explicit HID collections, recoverable diagnostics and an all-host
synthetic fixture require no frontend framework. Default output omits interface
paths/serials and limits HID previews to 32 bytes. Newly attached devices print
the same descriptor fields as initial enumeration, even when their first packet
is rejected after attachment.

## Observed verification

- Portable packet/state tests: 12 integration and 3 private boundary tests pass
  in debug and release on Linux; two public doctests pass.
- Rust 1.83 Windows GNU target: all-target compilation, strict Clippy and linked
  test/inspector binaries pass. Native unsafe remains confined to Windows FFI.
- Isolated Windows Server guest: six native API integration tests and five
  platform unit tests pass, covering QPC, enumeration, handle failures,
  registration conflicts, cleanup, array lengths and retry bounds. After the
  registration review fix, all six native API tests pass again on the rebuilt
  executable, including 4,097 repeated requests for one class.
- Production inspector in the interactive guest: runtime source 1, native
  keyboard handle `0x10041`, canonical A `0x07:0x04`, Down/Up sequence 3/4,
  native codes 30/65566, QPC frequency 10,000,000 and posted-message metadata
  are captured. Alt+F4 exits with code 0 and registration cleanup.
- Linux fixture output and ten negative CLI cases pass. Native execution from
  a noninteractive guest session also exits normally, with no input devices;
  that run alone did not establish acquisition. Windows fixture output and
  twelve negative CLI cases also pass. The rebuilt inspector passes a finite
  Windows guest smoke run with zero acquisitions; that does not prove the new
  arrival-output branch with live input.

This is actual Windows API/input execution through a Hyper-V virtual keyboard.
Physical latency/jitter and physical HID hardware were not measured. Declared
Linux/Windows/macOS CI and Windows MSRV jobs are not claimed as executed here.
Independent code/security review and full QA are separate task close gates.

The full runtime goal continues with chart compilation, judging, audio,
integration, rendering primitives, replay and the remaining platform/adapter
phases. This change does not complete that goal.
