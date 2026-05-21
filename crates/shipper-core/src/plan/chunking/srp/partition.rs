/// Partition input items into contiguous chunks of `batch_size`.
pub(crate) fn contiguous_chunks<T: Clone>(items: &[T], batch_size: usize) -> Vec<Vec<T>> {
    items.chunks(batch_size).map(<[T]>::to_vec).collect()
}
