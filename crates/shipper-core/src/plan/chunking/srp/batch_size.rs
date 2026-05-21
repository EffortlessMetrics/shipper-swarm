/// Normalize the requested max concurrency into a usable batch size.
///
/// A `max_concurrent` of `0` is treated as `1`.
pub(crate) fn normalize_batch_size(max_concurrent: usize) -> usize {
    max_concurrent.max(1)
}
