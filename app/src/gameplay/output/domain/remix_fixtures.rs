use super::*;

#[test]
fn finite_matrix_rows_parse_and_canonicalize_without_changing_coefficients() {
    for text in [
        "1;0.5",
        "1,0;0,1",
        " -1, 0.25 ; 1e-3, +2 ",
        "1e-45;3.4028235e38",
    ] {
        let matrix = parse_matrix(text).unwrap().unwrap();
        let encoded = matrix_text(&matrix).unwrap();
        let decoded = parse_matrix(&encoded).unwrap().unwrap();
        assert_eq!(decoded, matrix, "{text}");
        assert_eq!(encoded, matrix_text(&decoded).unwrap());
    }
    assert!(parse_matrix("").unwrap().is_none());
    assert!(parse_matrix("exact").unwrap().is_none());
}

#[test]
fn malformed_nonfinite_ragged_or_oversized_matrix_text_rejects() {
    for text in [
        ";",
        "1;",
        "1,,0",
        "1,0;1",
        "nan",
        "inf",
        "-inf",
        "1e999",
        "1\n;1",
        "1\u{2028};1",
        "exact;1",
    ] {
        assert!(parse_matrix(text).is_err(), "{text}");
    }
    assert!(parse_matrix(&vec!["1"; 33].join(";")).is_err());
    assert!(parse_matrix(&vec!["1"; 33].join(",")).is_err());
    assert!(parse_matrix(&"1".repeat(4097)).is_err());
    let full = vec![vec!["1"; 32].join(","); 32].join(";");
    let matrix = parse_matrix(&full).unwrap().unwrap();
    assert_eq!(
        (matrix.source_channels(), matrix.target_channels()),
        (32, 32)
    );
}

#[test]
fn shared_live_selection_preserves_and_resets_without_native_request_policy() {
    let matrix = parse_matrix("1;0.5").unwrap().unwrap();
    let (channels, same) = select_matrix(1, Some(&matrix), "").unwrap();
    assert_eq!(channels, 2);
    assert_eq!(same, Some(matrix));
    let (channels, empty) = select_matrix(1, same.as_ref(), "exact").unwrap();
    assert_eq!(channels, 1);
    assert!(empty.is_none());
    assert!(select_matrix(2, same.as_ref(), "").is_err());
    assert!(select_matrix(1, None, "1,0").is_err());
}
