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
