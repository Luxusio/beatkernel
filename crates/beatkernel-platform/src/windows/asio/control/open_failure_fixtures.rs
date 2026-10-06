//! Deferred SDK-gated scalar interpretation only; no bridge/driver calls.
use super::*;
#[test]
fn only_exact_retirement_flag_can_make_successful_close_evidence_available() {
    for flag in [0, 1, 2, u32::MAX] {
        let outcome = interpret_close_outcome(Status { domain: 0, code: 0 }, flag);
        assert_eq!(outcome.retired, flag == 1);
        if flag == 1 {
            assert!(outcome.error.is_none());
        } else {
            match outcome.error.unwrap() {
                AsioControlError::Native {
                    operation,
                    domain,
                    code,
                } => {
                    assert_eq!(operation, "callback retirement");
                    assert_eq!(domain, AsioControlErrorDomain::Bridge);
                    assert_eq!(code, 1);
                }
                _ => panic!("missing proof must be explicit bridge refusal"),
            }
        }
    }
}
#[test]
fn original_native_close_error_is_separate_from_retirement_and_preserves_domain_and_signed_code() {
    for raw_domain in [0, 1, 2, 3, 4, 99] {
        for code in [i32::MIN, 0, i32::MAX] {
            if raw_domain == 0 && code == 0 {
                continue;
            }
            let expected_domain = match raw_domain {
                1 => AsioControlErrorDomain::Com,
                2 => AsioControlErrorDomain::Asio,
                3 => AsioControlErrorDomain::Win32,
                4 => AsioControlErrorDomain::InitializationBoolean,
                _ => AsioControlErrorDomain::Bridge,
            };
            for flag in [0, 1, 2, u32::MAX] {
                let outcome = interpret_close_outcome(
                    Status {
                        domain: raw_domain,
                        code,
                    },
                    flag,
                );
                assert_eq!(outcome.retired, flag == 1);
                match outcome.error.unwrap() {
                    AsioControlError::Native {
                        operation,
                        domain,
                        code: actual,
                    } => {
                        assert_eq!(operation, "Release/CoUninitialize");
                        assert_eq!(domain, expected_domain);
                        assert_eq!(actual, code);
                    }
                    _ => panic!("original native status required regardless of retirement flag"),
                }
            }
        }
    }
}
