use std::collections::BTreeSet;
use std::io;

use zmin_git_core::object_store::PrefixOrFullObject;

use super::*;

const HEADS_PREFIX: &str = "refs/heads/";
const TAGS_PREFIX: &str = "refs/tags/";
const MAX_FETCH_REF_BYTES: usize = 4_096;

type ReachabilityResult<T> = std::result::Result<T, LfsReachabilityError>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct LfsRemoteFetchRefspec {
    negative: bool,
    source: String,
    destination: Option<String>,
    wildcard: bool,
}

impl LfsRemoteFetchRefspec {
    fn parse(value: &str) -> ReachabilityResult<Self> {
        let value = value.trim();
        if value.is_empty() {
            return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
        }
        let (force, value) = value
            .strip_prefix('+')
            .map_or((false, value), |value| (true, value));
        let (negative, value) = value
            .strip_prefix('^')
            .map_or((false, value), |value| (true, value));
        if force && negative {
            return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
        }
        let (source, destination) = if negative {
            if value.contains(':') {
                return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
            }
            (value, None)
        } else {
            value
                .split_once(':')
                .map_or((value, None), |(source, destination)| {
                    (source, Some(destination))
                })
        };
        if source.is_empty() || destination.is_some_and(str::is_empty) {
            return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
        }
        let source_wildcards = source.bytes().filter(|byte| *byte == b'*').count();
        let destination_wildcards = destination
            .map(|value| value.bytes().filter(|byte| *byte == b'*').count())
            .unwrap_or(0);
        if source_wildcards > 1
            || destination_wildcards > 1
            || destination.is_some() && source_wildcards != destination_wildcards
        {
            return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
        }
        validate_fetch_ref_pattern(source)?;
        if let Some(destination) = destination {
            validate_fetch_ref_pattern(destination)?;
        }
        Ok(Self {
            negative,
            source: source.to_owned(),
            destination: destination.map(str::to_owned),
            wildcard: source_wildcards == 1,
        })
    }

    fn is_negative_match(&self, source: &str) -> bool {
        self.negative && self.capture(source).is_some()
    }

    fn map_destination(&self, source: &str) -> Option<String> {
        if self.negative {
            return None;
        }
        let destination = self.destination.as_deref()?;
        let capture = self.capture(source)?;
        if self.wildcard {
            Some(destination.replacen('*', capture, 1))
        } else {
            Some(destination.to_owned())
        }
    }

    fn reverse_source(&self, destination: &str) -> Option<String> {
        if self.negative {
            return None;
        }
        let destination_pattern = self.destination.as_deref()?;
        let capture = capture_fetch_ref_pattern(destination_pattern, self.wildcard, destination)?;
        if self.wildcard {
            Some(self.source.replacen('*', capture, 1))
        } else {
            Some(self.source.clone())
        }
    }

    fn capture<'a>(&self, source: &'a str) -> Option<&'a str> {
        capture_fetch_ref_pattern(&self.source, self.wildcard, source)
    }
}

fn capture_fetch_ref_pattern<'a>(pattern: &str, wildcard: bool, value: &'a str) -> Option<&'a str> {
    if !wildcard {
        return (pattern == value).then_some("");
    }
    let (prefix, suffix) = pattern
        .split_once('*')
        .expect("validated fetch wildcard remains present");
    value
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
}

fn validate_fetch_ref_pattern(value: &str) -> ReachabilityResult<()> {
    if !value.starts_with("refs/") || value.len() > MAX_FETCH_REF_BYTES {
        return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
    }
    let candidate = value.replace('*', "wildcard");
    if !check_ref_format(&candidate, false) {
        return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
    }
    Ok(())
}

fn load_remote_fetch_refspecs(
    repo: &GitRepo,
    remote: Option<&str>,
) -> Result<Vec<LfsRemoteFetchRefspec>> {
    let Some(remote) = remote else {
        return Ok(Vec::new());
    };
    let entries = read_config_entries(repo).map_err(CliError::Io)?;
    entries
        .iter()
        .filter(|entry| {
            entry.section == "remote"
                && entry.subsection == remote
                && entry.key.eq_ignore_ascii_case("fetch")
        })
        .map(|entry| {
            LfsRemoteFetchRefspec::parse(&entry.value).map_err(|error| CliError::Stderr {
                code: 2,
                text: format!("error: {error}\n"),
            })
        })
        .collect()
}

/// One deterministic CLI-level LFS destination selection.
///
/// Endpoint discovery consumes this result instead of independently guessing
/// a default remote in each command. Multi-remote policy probing remains an
/// explicit, fail-closed boundary until that feature is implemented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CliLfsRemoteSelection {
    GlobalEndpoint,
    Named(String),
    FetchHeadEndpoint,
    PolicyRequired(CliLfsRemotePolicyRequirement),
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CliLfsRemotePolicyRequirement {
    pub(crate) autodetect: bool,
    pub(crate) search_all: bool,
}

impl CliLfsRemoteSelection {
    pub(crate) fn select(
        config: &LfsRuntimeConfig,
        operation: LfsOperation,
        requested: Option<&str>,
    ) -> Self {
        let inputs = config.endpoint_inputs();
        let has_global_endpoint = inputs.lfs_url.is_some()
            || inputs.lfsconfig.lfs_url().is_some()
            || (operation == LfsOperation::Push
                && (inputs.lfs_push_url.is_some() || inputs.lfsconfig.lfs_push_url().is_some()));
        if has_global_endpoint {
            return Self::GlobalEndpoint;
        }

        if let Some(remote) = requested {
            return inputs
                .remote(remote)
                .map(|_| Self::Named(remote.to_owned()))
                .unwrap_or(Self::Missing);
        }

        let policy = config.remote_policy();
        if policy.autodetect() || policy.search_all() {
            return Self::PolicyRequired(CliLfsRemotePolicyRequirement {
                autodetect: policy.autodetect(),
                search_all: policy.search_all(),
            });
        }

        if let Some(remote) = select_implicit_lfs_remote(operation, inputs) {
            return Self::Named(remote.name.clone());
        }
        if operation == LfsOperation::Fetch && inputs.fetch_head_url.is_some() {
            return Self::FetchHeadEndpoint;
        }
        Self::Missing
    }

    pub(crate) fn remote_name(&self) -> Option<&str> {
        match self {
            Self::Named(remote) => Some(remote),
            Self::GlobalEndpoint
            | Self::FetchHeadEndpoint
            | Self::PolicyRequired(_)
            | Self::Missing => None,
        }
    }
}

/// Zmin-native repository/ref adapter for the bounded LFS reachability planner.
///
/// This adapter never invokes an external `git` process. Object reads use the
/// packed-first store and pointer probes use its bounded prefix primitive.
pub(crate) struct CliLfsReachabilityRepository {
    store: LooseObjectStore,
    worktree_refs: RefStore,
    common_refs: RefStore,
    algorithm: GitHashAlgorithm,
    remote_fetch_refspecs: Vec<LfsRemoteFetchRefspec>,
}

impl CliLfsReachabilityRepository {
    pub(crate) fn new(repo: &GitRepo, remote: Option<&str>) -> Result<Self> {
        let algorithm = repo_hash_algorithm_from_config(repo).map_err(CliError::Io)?;
        let common_git_dir = read_common_git_dir(&repo.git_dir)?;
        let remote_fetch_refspecs = load_remote_fetch_refspecs(repo, remote)?;
        Ok(Self {
            store: LooseObjectStore::new(&repo.objects_dir, algorithm),
            worktree_refs: RefStore::new(&repo.git_dir, algorithm),
            common_refs: RefStore::new(&common_git_dir, algorithm),
            algorithm,
            remote_fetch_refspecs,
        })
    }

    fn resolve_ref_from(refs: &RefStore, name: &str) -> ReachabilityResult<Option<ObjectId>> {
        match refs.resolve(name) {
            Ok(object) => Ok(Some(object)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(LfsReachabilityError::Io(error.kind())),
        }
    }

    fn resolve_ref(&self, name: &str) -> ReachabilityResult<Option<ObjectId>> {
        if let Some(object) = Self::resolve_ref_from(&self.common_refs, name)? {
            return Ok(Some(object));
        }
        Self::resolve_ref_from(&self.worktree_refs, name)
    }

    fn resolve_head(&self) -> ReachabilityResult<Option<LfsResolvedRevision>> {
        match self.worktree_refs.read_head() {
            Ok(RefTarget::Direct(object)) => Ok(Some(LfsResolvedRevision::detached(object))),
            Ok(RefTarget::Symbolic(name)) => {
                let Some(object) = self.resolve_ref(&name)? else {
                    return Ok(None);
                };
                LfsResolvedRevision::referenced(object, name).map(Some)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(LfsReachabilityError::Io(error.kind())),
        }
    }

    fn resolve_short_ref(&self, revision: &str) -> ReachabilityResult<Option<LfsResolvedRevision>> {
        let candidates = [
            format!("{HEADS_PREFIX}{revision}"),
            format!("{TAGS_PREFIX}{revision}"),
        ];
        if candidates
            .iter()
            .any(|candidate| !check_ref_format(candidate, false))
        {
            return Err(LfsReachabilityError::UnsupportedRevision);
        }
        let mut resolved = Vec::new();
        for candidate in candidates {
            if let Some(object) = self.resolve_ref(&candidate)? {
                resolved.push(LfsResolvedRevision::referenced(object, candidate)?);
            }
        }
        match resolved.len() {
            0 => Ok(None),
            1 => Ok(resolved.pop()),
            _ => Err(LfsReachabilityError::AmbiguousRevision),
        }
    }

    fn resolve_mapped_remote_ref(&self, source: &str) -> ReachabilityResult<Option<ObjectId>> {
        if self
            .remote_fetch_refspecs
            .iter()
            .any(|refspec| refspec.is_negative_match(source))
        {
            return Ok(None);
        }
        let destinations = self
            .remote_fetch_refspecs
            .iter()
            .filter_map(|refspec| refspec.map_destination(source))
            .collect::<BTreeSet<_>>();
        let mut resolved = Vec::new();
        for destination in destinations {
            if let Some(object) = self.resolve_ref(&destination)? {
                resolved.push(object);
            }
        }
        resolved.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        resolved.dedup();
        match resolved.len() {
            0 => Ok(None),
            1 => Ok(resolved.pop()),
            _ => Err(LfsReachabilityError::AmbiguousPushDestination),
        }
    }

    fn resolve_fetch_batch_reference(
        &self,
        destination: &str,
    ) -> ReachabilityResult<Option<String>> {
        if destination.len() > MAX_FETCH_REF_BYTES
            || !destination.starts_with("refs/")
            || !check_ref_format(destination, false)
        {
            return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
        }

        let mut matched_destination = false;
        let mut sources = BTreeSet::new();
        for refspec in &self.remote_fetch_refspecs {
            let Some(source) = refspec.reverse_source(destination) else {
                continue;
            };
            matched_destination = true;
            if self
                .remote_fetch_refspecs
                .iter()
                .any(|candidate| candidate.is_negative_match(&source))
            {
                continue;
            }
            if source.len() > MAX_FETCH_REF_BYTES
                || !source.starts_with("refs/")
                || !check_ref_format(&source, false)
            {
                return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
            }
            sources.insert(source);
        }

        match sources.len() {
            0 if matched_destination => Ok(None),
            0 => Ok(Some(destination.to_owned())),
            1 => Ok(sources.into_iter().next()),
            _ => Err(LfsReachabilityError::AmbiguousFetchBatchRef),
        }
    }
}

impl GitObjectStore for CliLfsReachabilityRepository {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        self.store.packed_first().read_object(id)
    }

    fn read_object_prefix_or_full(
        &self,
        id: &ObjectId,
        max_bytes: usize,
    ) -> io::Result<PrefixOrFullObject> {
        self.store
            .packed_first()
            .read_object_prefix_or_full(id, max_bytes)
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        self.store.packed_first().object_header_hint(id)
    }
}

impl LfsReachabilityRepository for CliLfsReachabilityRepository {
    fn object_format(&self) -> GitHashAlgorithm {
        self.algorithm
    }

    fn read_lfs_object_bounded(
        &self,
        object: &ObjectId,
        max_bytes: usize,
    ) -> io::Result<PrefixOrFullObject> {
        self.store
            .packed_first()
            .read_object_prefix_or_full(object, max_bytes)
    }

    fn resolve_revision(&self, revision: &str) -> ReachabilityResult<Option<LfsResolvedRevision>> {
        if revision == "HEAD" {
            return self.resolve_head();
        }
        let hex_length = self.algorithm.digest_len() * 2;
        if revision.len() == hex_length && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let object = ObjectId::from_hex(self.algorithm, revision)
                .map_err(|_| LfsReachabilityError::InvalidRevision)?;
            return Ok(Some(LfsResolvedRevision::detached(object)));
        }
        if revision.starts_with("refs/") {
            if !check_ref_format(revision, false) {
                return Err(LfsReachabilityError::UnsupportedRevision);
            }
            return self
                .resolve_ref(revision)?
                .map(|object| LfsResolvedRevision::referenced(object, revision.to_owned()))
                .transpose();
        }
        self.resolve_short_ref(revision)
    }

    fn resolve_push_destination(&self, destination: &str) -> ReachabilityResult<Option<ObjectId>> {
        self.resolve_mapped_remote_ref(destination)
    }

    fn resolve_fetch_batch_ref(&self, reference: &str) -> ReachabilityResult<Option<String>> {
        self.resolve_fetch_batch_reference(reference)
    }

    fn current_fetch_revision(
        &self,
        _policy: LfsReachabilityRemotePolicy,
    ) -> ReachabilityResult<Option<LfsResolvedRevision>> {
        self.resolve_head()
    }

    fn visit_recent_fetch_roots(
        &self,
        _reference_cutoff_seconds: i64,
        _policy: LfsReachabilityRemotePolicy,
        _visitor: &mut dyn LfsReachabilityRootVisitor,
    ) -> ReachabilityResult<()> {
        Err(LfsReachabilityError::UnsupportedFetchSelection)
    }

    fn visit_all_fetch_roots(
        &self,
        _policy: LfsReachabilityRemotePolicy,
        _visitor: &mut dyn LfsReachabilityRootVisitor,
    ) -> ReachabilityResult<()> {
        Err(LfsReachabilityError::UnsupportedFetchSelection)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    struct AdapterFixture {
        _temporary: TempDir,
        repository: CliLfsReachabilityRepository,
        refs: RefStore,
    }

    struct DiscardRootVisitor;

    fn config_entry(name: &str, value: &str) -> ConfigEntry {
        let (section, subsection, key) = parse_config_name(name).expect("config key");
        ConfigEntry {
            raw_section: section.clone(),
            section,
            subsection,
            raw_key: key.clone(),
            key,
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: ConfigScope::Local,
            origin: "test".to_owned(),
            line: None,
        }
    }

    fn selector_config(entries: &[ConfigEntry]) -> (TempDir, LfsRuntimeConfig) {
        selector_config_with_fetch_head(entries, None)
    }

    fn selector_config_with_fetch_head(
        entries: &[ConfigEntry],
        fetch_head: Option<&str>,
    ) -> (TempDir, LfsRuntimeConfig) {
        let temporary = TempDir::new().expect("selector repository");
        let git_dir = temporary.path().join(".git");
        fs::create_dir_all(&git_dir).expect("selector Git directory");
        let config = LfsRuntimeConfig::load(LfsRuntimeConfigInput {
            git_dir: &git_dir,
            default_storage_git_dir: &git_dir,
            lfsconfig: LfsConfigSources::new(None, None, None),
            entries,
            branch: Some("main"),
            requested_remote: None,
            skip_smudge: None,
            skip_download_errors: None,
            http_environment: LfsHttpEnvironmentSnapshot::default(),
            fetch_head,
            url_rewriter: None,
        })
        .expect("selector config");
        (temporary, config)
    }

    impl LfsReachabilityRootVisitor for DiscardRootVisitor {
        fn visit(&mut self, _root: LfsRevisionRoot) -> ReachabilityResult<()> {
            Ok(())
        }
    }

    impl AdapterFixture {
        fn new(algorithm: GitHashAlgorithm, refspecs: &[&str]) -> Self {
            let temporary = TempDir::new().expect("temporary repository");
            let git_dir = temporary.path().join(".git");
            fs::create_dir_all(git_dir.join("objects")).expect("repository object directory");
            let refs = RefStore::new_without_environment(&git_dir, algorithm);
            let repository = CliLfsReachabilityRepository {
                store: LooseObjectStore::new(git_dir.join("objects"), algorithm),
                worktree_refs: refs.clone(),
                common_refs: refs.clone(),
                algorithm,
                remote_fetch_refspecs: refspecs
                    .iter()
                    .map(|value| LfsRemoteFetchRefspec::parse(value).expect("fixture refspec"))
                    .collect(),
            };
            Self {
                _temporary: temporary,
                repository,
                refs,
            }
        }

        fn write_ref(&self, name: &str, digit: char) -> ObjectId {
            let object = ObjectId::from_hex(
                self.repository.algorithm,
                &digit
                    .to_string()
                    .repeat(self.repository.algorithm.digest_len() * 2),
            )
            .expect("fixture object id");
            self.refs.write_ref(name, &object).expect("fixture ref");
            object
        }

        fn resolve(&self, revision: &str) -> ReachabilityResult<Option<LfsResolvedRevision>> {
            self.repository.resolve_revision(revision)
        }
    }

    #[test]
    fn configured_fetch_refspec_maps_heads_tags_and_custom_destinations() {
        let heads = LfsRemoteFetchRefspec::parse("+refs/heads/*:refs/remotes/upstream/custom/*")
            .expect("heads mapping");
        assert_eq!(
            heads.map_destination("refs/heads/topic"),
            Some("refs/remotes/upstream/custom/topic".to_owned())
        );
        assert_eq!(heads.map_destination("refs/tags/v1"), None);
        assert_eq!(
            heads.reverse_source("refs/remotes/upstream/custom/topic"),
            Some("refs/heads/topic".to_owned())
        );

        let tags =
            LfsRemoteFetchRefspec::parse("refs/tags/*:refs/remote-tags/*").expect("tag mapping");
        assert_eq!(
            tags.map_destination("refs/tags/v1"),
            Some("refs/remote-tags/v1".to_owned())
        );
        assert_eq!(
            tags.reverse_source("refs/remote-tags/v1"),
            Some("refs/tags/v1".to_owned())
        );
    }

    #[test]
    fn configured_fetch_refspec_rejects_malformed_and_honors_negative_patterns() {
        let negative =
            LfsRemoteFetchRefspec::parse("^refs/heads/private/*").expect("negative mapping");
        assert!(negative.is_negative_match("refs/heads/private/key"));
        assert!(!negative.is_negative_match("refs/heads/public/key"));
        for invalid in [
            "+^refs/heads/*",
            "refs/heads/*:refs/remotes/origin/main",
            "refs/heads/**:refs/remotes/origin/**",
            "refs/heads/main:",
            "heads/main:refs/remotes/origin/main",
        ] {
            assert_eq!(
                LfsRemoteFetchRefspec::parse(invalid),
                Err(LfsReachabilityError::InvalidRemoteFetchRefspec)
            );
        }
    }

    #[test]
    fn revision_resolver_accepts_only_bounded_current_forms() {
        let fixture = AdapterFixture::new(GitHashAlgorithm::Sha1, &[]);
        let main = fixture.write_ref("refs/heads/main", '1');
        fixture
            .refs
            .write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("symbolic HEAD");

        for revision in ["HEAD", "main", "refs/heads/main"] {
            let resolved = fixture
                .resolve(revision)
                .expect("bounded revision")
                .expect("resolved revision");
            assert_eq!(resolved.object(), &main);
            assert_eq!(resolved.reference(), Some("refs/heads/main"));
        }
        let detached = fixture
            .resolve(&main.to_hex())
            .expect("full object id")
            .expect("detached revision");
        assert_eq!(detached.object(), &main);
        assert_eq!(detached.reference(), None);

        for revision in ["HEAD~1", "main^", ":/message", "main:path", "main@{1}"] {
            assert_eq!(
                fixture.resolve(revision),
                Err(LfsReachabilityError::UnsupportedRevision)
            );
        }
    }

    #[test]
    fn current_fetch_revision_preserves_symbolic_identity_and_detached_state() {
        let symbolic = AdapterFixture::new(GitHashAlgorithm::Sha1, &[]);
        let main = symbolic.write_ref("refs/heads/main", '1');
        symbolic
            .refs
            .write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("symbolic HEAD");
        let policy = LfsReachabilityRemotePolicy::new(false, false);
        let current = symbolic
            .repository
            .current_fetch_revision(policy)
            .expect("symbolic current revision")
            .expect("symbolic HEAD target");
        assert_eq!(current.object(), &main);
        assert_eq!(current.reference(), Some("refs/heads/main"));

        let detached = AdapterFixture::new(GitHashAlgorithm::Sha1, &[]);
        let object = detached.write_ref("HEAD", '2');
        let current = detached
            .repository
            .current_fetch_revision(policy)
            .expect("detached current revision")
            .expect("detached HEAD target");
        assert_eq!(current.object(), &object);
        assert_eq!(current.reference(), None);
    }

    #[test]
    fn fetch_batch_ref_reverses_selected_remote_destinations_and_preserves_server_refs() {
        let fixture = AdapterFixture::new(
            GitHashAlgorithm::Sha1,
            &[
                "+refs/heads/*:refs/remotes/upstream/custom/*",
                "+refs/tags/*:refs/remotes/upstream/tags/*",
                "^refs/heads/private/*",
            ],
        );
        assert_eq!(
            fixture
                .repository
                .resolve_fetch_batch_ref("refs/remotes/upstream/custom/topic"),
            Ok(Some("refs/heads/topic".to_owned()))
        );
        assert_eq!(
            fixture
                .repository
                .resolve_fetch_batch_ref("refs/remotes/upstream/tags/v1"),
            Ok(Some("refs/tags/v1".to_owned()))
        );
        assert_eq!(
            fixture
                .repository
                .resolve_fetch_batch_ref("refs/heads/local"),
            Ok(Some("refs/heads/local".to_owned()))
        );
        assert_eq!(
            fixture
                .repository
                .resolve_fetch_batch_ref("refs/remotes/upstream/custom/private/key"),
            Ok(None)
        );
    }

    #[test]
    fn ambiguous_reverse_fetch_mapping_fails_closed() {
        let fixture = AdapterFixture::new(
            GitHashAlgorithm::Sha1,
            &[
                "+refs/heads/*:refs/remotes/upstream/*",
                "+refs/changes/*:refs/remotes/upstream/*",
            ],
        );
        assert_eq!(
            fixture
                .repository
                .resolve_fetch_batch_ref("refs/remotes/upstream/topic"),
            Err(LfsReachabilityError::AmbiguousFetchBatchRef)
        );
    }

    #[test]
    fn short_revision_is_ambiguous_between_heads_and_tags() {
        let fixture = AdapterFixture::new(GitHashAlgorithm::Sha256, &[]);
        fixture.write_ref("refs/heads/release", '1');
        fixture.write_ref("refs/tags/release", '2');
        assert_eq!(
            fixture.resolve("release"),
            Err(LfsReachabilityError::AmbiguousRevision)
        );
    }

    #[test]
    fn push_destination_uses_configured_mapping_and_negative_exclusions() {
        let mapped = AdapterFixture::new(
            GitHashAlgorithm::Sha1,
            &[
                "+refs/heads/*:refs/remotes/upstream/custom/*",
                "+refs/tags/*:refs/remotes/upstream/tags/*",
            ],
        );
        let remote = mapped.write_ref("refs/remotes/upstream/custom/main", '3');
        assert_eq!(
            mapped
                .repository
                .resolve_push_destination("refs/heads/main")
                .expect("mapped destination"),
            Some(remote)
        );
        let tag = mapped.write_ref("refs/remotes/upstream/tags/v1", '7');
        assert_eq!(
            mapped
                .repository
                .resolve_push_destination("refs/tags/v1")
                .expect("mapped tag destination"),
            Some(tag)
        );

        let excluded = AdapterFixture::new(
            GitHashAlgorithm::Sha1,
            &[
                "+refs/heads/*:refs/remotes/upstream/custom/*",
                "^refs/heads/private/*",
            ],
        );
        excluded.write_ref("refs/remotes/upstream/custom/private/key", '4');
        assert_eq!(
            excluded
                .repository
                .resolve_push_destination("refs/heads/private/key")
                .expect("negative destination"),
            None
        );
    }

    #[test]
    fn conflicting_fetch_mappings_fail_closed() {
        let fixture = AdapterFixture::new(
            GitHashAlgorithm::Sha1,
            &[
                "+refs/heads/*:refs/remotes/one/*",
                "+refs/heads/*:refs/remotes/two/*",
            ],
        );
        fixture.write_ref("refs/remotes/one/main", '5');
        fixture.write_ref("refs/remotes/two/main", '6');
        assert_eq!(
            fixture
                .repository
                .resolve_push_destination("refs/heads/main"),
            Err(LfsReachabilityError::AmbiguousPushDestination)
        );
    }

    #[test]
    fn recent_and_all_are_explicitly_unsupported_in_the_current_adapter() {
        let fixture = AdapterFixture::new(GitHashAlgorithm::Sha1, &[]);
        let policy = LfsReachabilityRemotePolicy::new(false, false);
        let mut visitor = DiscardRootVisitor;
        assert_eq!(
            fixture
                .repository
                .visit_recent_fetch_roots(0, policy, &mut visitor),
            Err(LfsReachabilityError::UnsupportedFetchSelection)
        );
        assert_eq!(
            fixture
                .repository
                .visit_all_fetch_roots(policy, &mut visitor),
            Err(LfsReachabilityError::UnsupportedFetchSelection)
        );
    }

    #[test]
    fn cli_remote_selector_uses_one_pinned_precedence_chain() {
        let global_entries = [config_entry("lfs.url", "https://example.test/lfs")];
        let (_, global) = selector_config(&global_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&global, LfsOperation::Fetch, Some("missing")),
            CliLfsRemoteSelection::GlobalEndpoint
        );
        let global_push_entries = [config_entry("lfs.pushurl", "https://example.test/push-lfs")];
        let (_, global_push) = selector_config(&global_push_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&global_push, LfsOperation::Push, Some("missing")),
            CliLfsRemoteSelection::GlobalEndpoint
        );

        let default_entries = [
            config_entry("remote.preferred.url", "https://preferred.test/repo.git"),
            config_entry("remote.tracking.url", "https://tracking.test/repo.git"),
            config_entry("remote.lfsdefault", "preferred"),
            config_entry("branch.main.remote", "tracking"),
        ];
        let (_, tracking) = selector_config(&default_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&tracking, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("tracking".to_owned())
        );

        let lfs_default_entries = [
            config_entry("remote.preferred.url", "https://preferred.test/repo.git"),
            config_entry("remote.origin.url", "https://origin.test/repo.git"),
            config_entry("remote.lfsdefault", "preferred"),
        ];
        let (_, lfs_default) = selector_config(&lfs_default_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&lfs_default, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("preferred".to_owned())
        );

        let push_entries = [
            config_entry("remote.push.url", "https://push.test/repo.git"),
            config_entry("remote.tracking.url", "https://tracking.test/repo.git"),
            config_entry("remote.lfspushdefault", "push"),
            config_entry("branch.main.pushremote", "tracking"),
        ];
        let (_, branch_push) = selector_config(&push_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&branch_push, LfsOperation::Push, None),
            CliLfsRemoteSelection::Named("tracking".to_owned())
        );

        let push_default_entries = [
            config_entry("remote.push.url", "https://push.test/repo.git"),
            config_entry("remote.origin.url", "https://origin.test/repo.git"),
            config_entry("remote.lfspushdefault", "push"),
        ];
        let (_, push_default) = selector_config(&push_default_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&push_default, LfsOperation::Push, None),
            CliLfsRemoteSelection::Named("push".to_owned())
        );

        let sole_entries = [config_entry(
            "remote.upstream.url",
            "https://upstream.test/repo.git",
        )];
        let (_, sole) = selector_config(&sole_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&sole, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("upstream".to_owned())
        );

        let tracking_entries = [
            config_entry("remote.origin.url", "https://origin.test/repo.git"),
            config_entry("remote.tracking.url", "https://tracking.test/repo.git"),
            config_entry("branch.main.remote", "tracking"),
        ];
        let (_, tracking) = selector_config(&tracking_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&tracking, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("tracking".to_owned())
        );

        let origin_entries = [
            config_entry("remote.origin.url", "https://origin.test/repo.git"),
            config_entry("remote.backup.url", "https://backup.test/repo.git"),
        ];
        let (_, origin) = selector_config(&origin_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&origin, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("origin".to_owned())
        );

        let missing_branch_sole_entries = [
            config_entry("remote.upstream.url", "https://upstream.test/repo.git"),
            config_entry("branch.main.remote", "missing"),
        ];
        let (_, missing_branch_sole) = selector_config(&missing_branch_sole_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&missing_branch_sole, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("upstream".to_owned())
        );

        let endpointless_default_origin_entries = [
            config_entry("remote.empty.fetch", "+refs/heads/*:refs/remotes/empty/*"),
            config_entry("remote.origin.url", "https://origin.test/repo.git"),
            config_entry("branch.main.remote", "missing"),
            config_entry("remote.lfsdefault", "empty"),
        ];
        let (_, endpointless_default_origin) =
            selector_config(&endpointless_default_origin_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&endpointless_default_origin, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::Named("origin".to_owned())
        );

        let missing_push_sole_entries = [
            config_entry("remote.upstream.url", "https://upstream.test/repo.git"),
            config_entry("branch.main.pushremote", "missing-push"),
            config_entry("remote.lfspushdefault", "missing-default"),
            config_entry("branch.main.remote", "missing-fetch"),
        ];
        let (_, missing_push_sole) = selector_config(&missing_push_sole_entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&missing_push_sole, LfsOperation::Push, None),
            CliLfsRemoteSelection::Named("upstream".to_owned())
        );

        let endpointless_entries = [
            config_entry("remote.empty.fetch", "+refs/heads/*:refs/remotes/empty/*"),
            config_entry("branch.main.remote", "empty"),
        ];
        let fetch_head = concat!(
            "1111111111111111111111111111111111111111\t\t",
            "branch 'main' of https://fetch.test/repo.git\n"
        );
        let (_, fetch_head_config) =
            selector_config_with_fetch_head(&endpointless_entries, Some(fetch_head));
        assert_eq!(
            CliLfsRemoteSelection::select(&fetch_head_config, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::FetchHeadEndpoint
        );
    }

    #[test]
    fn cli_remote_selector_exposes_unimplemented_multi_remote_policy() {
        let entries = [
            config_entry("remote.origin.url", "https://example.test/repo.git"),
            config_entry("lfs.remote.autodetect", "true"),
            config_entry("lfs.remote.searchall", "true"),
        ];
        let (_, config) = selector_config(&entries);
        assert_eq!(
            CliLfsRemoteSelection::select(&config, LfsOperation::Fetch, None),
            CliLfsRemoteSelection::PolicyRequired(CliLfsRemotePolicyRequirement {
                autodetect: true,
                search_all: true,
            })
        );
        assert_eq!(
            CliLfsRemoteSelection::select(&config, LfsOperation::Fetch, Some("origin")),
            CliLfsRemoteSelection::Named("origin".to_owned())
        );
        assert_eq!(
            CliLfsRemoteSelection::select(
                &config,
                LfsOperation::Push,
                Some("https://example.test/repo.git")
            ),
            CliLfsRemoteSelection::Missing
        );
    }
}
