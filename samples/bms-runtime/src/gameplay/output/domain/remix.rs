//! Explicit business-owned channel choice without native device operations.
use beatkernel::audio::ChannelMatrix;

#[derive(Clone, Debug, PartialEq)]
pub struct RemixedOutputRequest<R> {
    pub native: R,
    pub matrix: Option<ChannelMatrix>,
}
impl<R> RemixedOutputRequest<R> {
    pub const fn strict(native: R) -> Self {
        Self {
            native,
            matrix: None,
        }
    }
    pub fn remixed(native: R, matrix: ChannelMatrix) -> Self {
        Self {
            native,
            matrix: Some(matrix),
        }
    }
}

pub const MAX_MATRIX_TEXT_BYTES: usize = 4096;
/// Rows are target channels, comma-delimited columns are original source channels.
pub fn parse_matrix(text: &str) -> Result<Option<ChannelMatrix>, String> {
    if text.len() > MAX_MATRIX_TEXT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("channel matrix exceeds 4096 bytes or contains controls".into());
    }
    let text = text.trim();
    if text.is_empty() || text == "exact" {
        return Ok(None);
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(1024)
        .map_err(|_| "channel matrix allocation failed")?;
    let mut source = None;
    let mut targets = 0;
    for row in text.split(';') {
        targets += 1;
        if targets > 32 {
            return Err("channel matrix has more than 32 rows".into());
        }
        let mut columns = 0;
        for token in row.split(',') {
            columns += 1;
            if columns > 32 {
                return Err("channel matrix has more than 32 columns".into());
            }
            let value: f32 = token
                .trim()
                .parse()
                .map_err(|_| "channel matrix coefficients must be finite f32 numbers")?;
            if !value.is_finite() {
                return Err("channel matrix coefficients must be finite f32 numbers".into());
            }
            values.push(value);
        }
        if source.is_some_and(|source| source != columns) {
            return Err("channel matrix rows must have equal widths".into());
        }
        source = Some(columns);
    }
    let matrix = ChannelMatrix::new(source.unwrap() as u16, targets as u16, &values)
        .map_err(|e| e.to_string())?;
    matrix_text(&matrix)?; // Prove successful replies can represent the exact accepted policy.
    Ok(Some(matrix))
}
/// Shortest display/scientific spelling is round-trippable to the accepted f32.
pub fn matrix_text(matrix: &ChannelMatrix) -> Result<String, String> {
    let mut output = String::new();
    for (row, values) in matrix
        .coefficients()
        .chunks_exact(usize::from(matrix.source_channels()))
        .enumerate()
    {
        if row != 0 {
            output.push(';');
        }
        for (column, value) in values.iter().enumerate() {
            if column != 0 {
                output.push(',');
            }
            let decimal = value.to_string();
            let scientific = format!("{value:e}");
            output.push_str(if scientific.len() < decimal.len() {
                &scientific
            } else {
                &decimal
            });
        }
        if output.len() > MAX_MATRIX_TEXT_BYTES {
            return Err("channel matrix canonical text exceeds 4096 bytes".into());
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "remix_fixtures.rs"]
mod fixtures;
