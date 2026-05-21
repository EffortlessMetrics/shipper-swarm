/// Normalize a requested max concurrency into a valid batch size.
///
/// A value of `0` is treated as `1`.
pub(super) fn normalized_batch_size(max_concurrent: usize) -> usize {
    max_concurrent.max(1)
}
