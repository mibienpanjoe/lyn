/// Only the exact startup argument suppresses Library presentation. It never
/// invokes capture or supplies context; helper modes dispatch before GUI setup.
pub(crate) fn is_background_launch(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == "--background")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_background_argument_hides_library() {
        let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_background_launch(&args(&["lyn", "--background"])));
        for values in [
            vec!["lyn"],
            vec!["--background"],
            vec!["lyn", "--background=false"],
            vec!["lyn", "/tmp/--background"],
        ] {
            assert!(!is_background_launch(&args(&values)));
        }
    }
}
