//! Bound common-ancestor searches by the indexed tip, including a shorter node chain.
pub fn ancestor_heights(
    last_indexed: u32,
    node_tip: u32,
    max_depth: u32,
) -> Result<Vec<u32>, String> {
    let oldest = last_indexed.saturating_sub(max_depth);
    let newest = last_indexed.min(node_tip);
    if newest < oldest || max_depth == 0 {
        return Err(format!(
            "Reorg deeper than {max_depth} blocks — manual intervention required"
        ));
    }
    Ok((oldest..=newest).rev().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shorter_tip_never_requests_future_heights_and_counts_removed_tail() {
        assert_eq!(ancestor_heights(100, 99, 3).unwrap(), vec![99, 98, 97]);
        assert_eq!(
            ancestor_heights(100, 101, 3).unwrap(),
            vec![100, 99, 98, 97]
        );
        assert_eq!(ancestor_heights(100, 97, 3).unwrap(), vec![97]);
        assert!(ancestor_heights(100, 96, 3).is_err());
        assert_eq!(ancestor_heights(1, 0, 5).unwrap(), vec![0]);
    }
}
