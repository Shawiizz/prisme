/// Whether version `a` (`1.2.3`) comes after `b`, number by number.
pub fn newer(a: &str, b: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> { v.split('.').map(|n| n.parse().unwrap_or(0)).collect() };
    let (a, b) = (numbers(a), numbers(b));
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    let order = (0..a.len().max(b.len())).map(|i| at(&a, i).cmp(&at(&b, i))).find(|o| o.is_ne());
    order == Some(std::cmp::Ordering::Greater)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_newer_number_by_number_not_by_its_text() {
        assert!(newer("0.3.10", "0.3.9"));
        assert!(newer("0.4.0", "0.3.12"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.3.6", "0.3.6"));
        assert!(!newer("0.3.5", "0.3.6"));
    }
}
