pub fn chunk_values<T: Clone>(values: &[T], chunk_size: usize) -> Vec<Vec<T>> {
    if chunk_size == 0 {
        return if values.is_empty() {
            Vec::new()
        } else {
            vec![values.to_vec()]
        };
    }
    values.chunks(chunk_size).map(<[T]>::to_vec).collect()
}

#[cfg(test)]
mod tests {
    use super::chunk_values;

    #[test]
    fn chunk_values_respects_batch_size() {
        let values = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            chunk_values(&values, 2),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()]
            ]
        );
    }

    #[test]
    fn chunk_values_zero_batch_returns_single_chunk() {
        let values = vec![1, 2, 3];
        assert_eq!(chunk_values(&values, 0), vec![vec![1, 2, 3]]);
        assert!(chunk_values::<i32>(&[], 2).is_empty());
    }
}
