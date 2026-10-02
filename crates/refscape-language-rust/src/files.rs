pub const POLICY: refscape_language_support::catalog::WalkPolicy =
    refscape_language_support::catalog::WalkPolicy {
        extensions: &["rs"],
        excluded: &["target", ".git", ".hg", ".svn", "node_modules"],
        case_insensitive: false,
        symlink_files: false,
        exclude_virtual_environments: false,
        canonical_paths: false,
    };
