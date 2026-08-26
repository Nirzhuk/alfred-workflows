use super::*;
use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
use crate::agent_accounts::runtime_profile::{
    RuntimeProfile, RuntimeProfileBinding, RuntimeProfileStore,
};
use crate::agents::managed_runtime::ManagedRuntimeSupervisor;
use crate::agents::native::NativeRuntimeRegistry;
use crate::agents::runtime_package::{
    RuntimePackageExpectation, RuntimePackageSelection, RuntimePackageStore,
    RuntimePackageVerification,
};
use crate::agents::OpaqueAgentAccountRef;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

struct Fixture {
    selection: RuntimePackageSelection,
    profile_store: RuntimeProfileStore,
    app_data: PathBuf,
    executable: PathBuf,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(build_fixture)
}

fn build_fixture() -> Fixture {
    let app_data = std::env::temp_dir().join(format!(
        "alfred-claude-managed-fixture-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let source = app_data.join("source");
    fs::create_dir_all(source.join("bin")).expect("fixture bin");
    fs::create_dir_all(source.join("legal")).expect("fixture legal");
    let target = fixture_target();
    let artifact = artifact_for_target(target).expect("supported fixture target");
    let executable = source.join("bin").join(artifact.executable_name);
    fs::copy(
        std::env::current_exe().expect("test executable"),
        &executable,
    )
    .expect("copy fixture binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fixture executable mode");
    }
    fs::write(
        source.join(CLAUDE_CODE_LICENSE_RESOURCE),
        CLAUDE_CODE_LICENSE_BYTES,
    )
    .expect("fixture license");
    fs::write(
        source.join(CLAUDE_CODE_NOTICE_RESOURCE),
        CLAUDE_CODE_NOTICE_BYTES,
    )
    .expect("fixture notice");
    let mut manifest = package_manifest();
    let fixture_digest = format!(
        "{:x}",
        Sha256::digest(fs::read(&executable).expect("fixture executable bytes"))
    );
    manifest
        .targets
        .iter_mut()
        .find(|candidate| candidate.target == target)
        .expect("fixture manifest target")
        .executable
        .sha256 = fixture_digest;
    let expectation =
        RuntimePackageExpectation::for_product(AgentProductId::ClaudeCodeSubscription, target)
            .expect("fixture expectation");
    let verification = RuntimePackageVerification::verified_fixture(manifest, expectation)
        .expect("fixture sealed verification");
    let package_store = RuntimePackageStore::open(&app_data).expect("fixture package store");
    package_store
        .stage_and_activate(&source, &verification, None)
        .expect("activate fixture");
    let selection = package_store
        .select_active(&verification)
        .expect("select fixture");
    let executable = selection
        .verified_active_executable_path()
        .expect("active fixture executable");
    let profile_store = RuntimeProfileStore::new(&app_data).expect("fixture profile store");
    Fixture {
        selection,
        profile_store,
        app_data: fs::canonicalize(app_data).expect("canonical fixture root"),
        executable,
    }
}

fn fixture_target() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "musl"))]
    return "aarch64-unknown-linux-musl";
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "musl"))]
    return "x86_64-unknown-linux-musl";
    #[cfg(all(windows, target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";
    #[cfg(all(windows, target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";
    #[allow(unreachable_code)]
    panic!("unsupported fixture target")
}

fn profile(fixture: &Fixture) -> RuntimeProfile {
    let account = OpaqueAgentAccountRef::parse(&format!(
        "account_claude_fixture_{}",
        uuid::Uuid::new_v4().simple()
    ))
    .expect("fixture account");
    let binding = RuntimeProfileBinding::new(
        &account,
        AgentProductId::ClaudeCodeSubscription,
        ManagedRuntimeId::ClaudeCodeManaged,
        CLAUDE_CODE_RUNTIME_VERSION,
    )
    .expect("fixture binding");
    fixture
        .profile_store
        .create(&binding)
        .expect("fixture profile")
}

struct FixturePublisherVerifier;

impl ClaudePublisherVerifier for FixturePublisherVerifier {
    fn verify(
        &self,
        request: ClaudePublisherVerificationRequest<'_>,
    ) -> Result<RuntimePackageVerification, ClaudePublisherVerificationError> {
        Ok(RuntimePackageVerification::verified_fixture(
            request.package_manifest.clone(),
            request.expectation.clone(),
        )
        .expect("fixture publisher verification"))
    }
}

struct MismatchedPublisherVerifier;

impl ClaudePublisherVerifier for MismatchedPublisherVerifier {
    fn verify(
        &self,
        request: ClaudePublisherVerificationRequest<'_>,
    ) -> Result<RuntimePackageVerification, ClaudePublisherVerificationError> {
        let mut manifest = request.package_manifest.clone();
        manifest.targets[0].executable.sha256 = "0".repeat(64);
        Ok(
            RuntimePackageVerification::verified_fixture(manifest, request.expectation.clone())
                .expect("mismatched fixture remains structurally valid"),
        )
    }
}

#[test]
fn package_freeze_matches_signed_publisher_release_manifest() {
    let manifest = package_manifest();
    assert_eq!(manifest.runtime_version, "2.1.246");
    assert_eq!(manifest.targets.len(), 8);
    assert!(!manifest.update_policy.self_update_allowed);
    assert!(!manifest.update_policy.path_lookup_allowed);
    assert_eq!(
        format!("{:x}", Sha256::digest(CLAUDE_CODE_LICENSE_BYTES)),
        CLAUDE_CODE_LICENSE_SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(CLAUDE_CODE_NOTICE_BYTES)),
        CLAUDE_CODE_NOTICE_SHA256
    );
    for artifact in CLAUDE_CODE_RELEASE_ARTIFACTS {
        assert_eq!(
            artifact.url(),
            format!(
                "{}/{}/{}",
                CLAUDE_CODE_RELEASE_BASE_URL, artifact.publisher_platform, artifact.executable_name
            )
        );
    }
    let sealed = verify_package_for_install(
        &fixture().app_data,
        fixture_target(),
        &official_release_manifest_fixture(),
        b"fixture detached signature",
        &FixturePublisherVerifier,
    )
    .expect("publisher manifest accepted by sealed fixture verifier");
    assert_eq!(
        sealed.expectation().product(),
        AgentProductId::ClaudeCodeSubscription
    );
}

#[test]
fn publisher_manifest_or_package_mismatch_fails_before_activation() {
    let mut raw: serde_json::Value =
        serde_json::from_slice(&official_release_manifest_fixture()).expect("manifest JSON");
    raw["platforms"]["darwin-arm64"]["checksum"] = serde_json::json!("0".repeat(64));
    let error = verify_package_for_install(
        &fixture().app_data,
        fixture_target(),
        &serde_json::to_vec(&raw).expect("mutated manifest"),
        b"fixture detached signature",
        &FixturePublisherVerifier,
    )
    .expect_err("publisher mismatch rejected");
    assert_eq!(
        error.code(),
        ClaudePackageErrorCode::ReleaseManifestMismatch
    );

    let missing_signature = verify_package_for_install(
        &fixture().app_data,
        fixture_target(),
        &official_release_manifest_fixture(),
        b"",
        &FixturePublisherVerifier,
    )
    .expect_err("signature required");
    assert_eq!(
        missing_signature.code(),
        ClaudePackageErrorCode::DetachedSignatureMissing
    );

    let package_mismatch = verify_package_for_install(
        &fixture().app_data,
        fixture_target(),
        &official_release_manifest_fixture(),
        b"fixture detached signature",
        &MismatchedPublisherVerifier,
    )
    .expect_err("sealed verifier package mismatch rejected");
    assert_eq!(
        package_mismatch.code(),
        ClaudePackageErrorCode::SealedVerificationMismatch
    );
}

#[test]
fn production_registration_and_custom_renderer_remain_separately_blocked() {
    let error = register_subscription(&NativeRuntimeRegistry::default())
        .expect_err("subscription registration blocked");
    for blocker in subscription_registration_blockers() {
        assert!(error.message.contains(blocker));
    }
    assert!(subscription_release_gates().iter().any(|gate| {
        gate.gate == "native_workflow_renderer"
            && gate.evidence == WORKFLOW_RENDERER_APPROVAL_BLOCKED_CODE
    }));
    assert_eq!(
        ClaudeTerminalMode::AuthLogin.arguments(),
        vec!["auth".to_string(), "login".to_string()]
    );
    assert_eq!(
        ClaudeTerminalMode::AuthLogout.arguments(),
        vec!["auth".to_string(), "logout".to_string()]
    );
    assert!(ClaudeTerminalMode::Onboarding.arguments().is_empty());
    assert!(ClaudeTerminalMode::Interactive.arguments().is_empty());
}

#[test]
fn auth_status_discloses_api_key_precedence_instead_of_claiming_subscription() {
    let subscription = parse_auth_status(
        br#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","apiKeySource":"/login managed key","subscriptionType":"max"}"#,
    )
    .expect("subscription status");
    assert!(subscription.is_subscription_billed());
    assert!(!subscription.api_key_takes_precedence());

    let overridden = parse_auth_status(
        br#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","apiKeySource":"ANTHROPIC_API_KEY","subscriptionType":"max"}"#,
    )
    .expect("overridden status");
    assert_eq!(
        overridden.billing_source,
        ClaudeBillingSource::EnvironmentApiKey
    );
    assert_eq!(
        overridden.billing_warning_code,
        Some(API_KEY_PRECEDENCE_WARNING_CODE)
    );
    assert!(!overridden.is_subscription_billed());
}

#[test]
fn managed_supervisor_status_handles_logged_in_logged_out_and_api_precedence() {
    let fixture = fixture();
    let service = ClaudeAuthStatusService::new(ManagedRuntimeSupervisor::new());
    let subscription = service
        .query_fixture(&fixture.selection, &profile(fixture), "status_subscription")
        .expect("subscription status");
    assert_eq!(
        subscription.billing_source,
        ClaudeBillingSource::ClaudeSubscription
    );

    let api_key = service
        .query_fixture(&fixture.selection, &profile(fixture), "status_api_key")
        .expect("api-key status");
    assert!(api_key.api_key_takes_precedence());

    let logged_out = service
        .query_fixture(&fixture.selection, &profile(fixture), "status_logged_out")
        .expect("logged-out status");
    assert!(!logged_out.logged_in);
    assert_eq!(
        logged_out.billing_source,
        ClaudeBillingSource::NotAuthenticated
    );
    assert_eq!(logged_out.subscription_type, None);
    assert_eq!(logged_out.identity, None);
}

fn start_fixture_session(fixture_mode: &str) -> ClaudeTerminalSession {
    let fixture = fixture();
    let profile = profile(fixture);
    super::terminal::with_terminal_fixture(fixture_mode, None, || {
        start_terminal_session(
            &fixture.selection,
            &profile,
            ClaudeTerminalLaunchSpec::new(ClaudeTerminalMode::Fixture, &fixture.app_data, 80, 24),
        )
        .expect("start fixture terminal")
    })
}

fn read_until(session: &ClaudeTerminalSession, needle: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut output = Vec::new();
    while Instant::now() < deadline && !output.windows(needle.len()).any(|part| part == needle) {
        if let Some(chunk) = session
            .read_output(Duration::from_millis(100))
            .expect("terminal output")
        {
            output.extend_from_slice(
                &BASE64_STANDARD
                    .decode(chunk.data_base64)
                    .expect("base64 terminal output"),
            );
        }
    }
    output
}

#[test]
fn first_login_terminal_relays_input_output_and_resize_without_parsing_codes() {
    let session = start_fixture_session("first_login");
    let opening = read_until(&session, b"Choose login method");
    assert!(opening
        .windows(19)
        .any(|part| part == b"Choose login method"));
    session.resize(120, 42, 1_200, 840).expect("resize PTY");
    session.write_input(b"1\r").expect("terminal input");
    let response = read_until(&session, b"provider-owned-browser-flow");
    assert!(response
        .windows(27)
        .any(|part| part == b"provider-owned-browser-flow"));
    session.cancel().expect("cancel onboarding");
}

#[test]
fn already_logged_in_terminal_remains_the_unmodified_interactive_surface() {
    let session = start_fixture_session("already_logged_in");
    assert!(read_until(&session, b"Claude Code ready")
        .windows(17)
        .any(|part| part == b"Claude Code ready"));
    let snapshot = session.cancel().expect("cancel interactive fixture");
    assert_eq!(snapshot.lifecycle, ClaudeTerminalLifecycle::Cancelled);
}

#[test]
fn status_logout_and_crash_are_terminal_and_bounded() {
    let logout = start_fixture_session("logout");
    let snapshot = logout.wait(Duration::from_secs(3)).expect("logout exit");
    assert_eq!(snapshot.lifecycle, ClaudeTerminalLifecycle::Exited);
    assert!(read_until(&logout, b"Logged out")
        .windows(10)
        .any(|part| part == b"Logged out"));

    let crash = start_fixture_session("crash");
    let snapshot = crash.wait(Duration::from_secs(3)).expect("crash exit");
    assert_eq!(snapshot.lifecycle, ClaudeTerminalLifecycle::Crashed);
    assert_eq!(snapshot.exit_code, Some(17));

    let flood = start_fixture_session("flood");
    let snapshot = flood.wait(Duration::from_secs(5)).expect("flood stopped");
    assert_eq!(
        snapshot.lifecycle,
        ClaudeTerminalLifecycle::OutputLimitExceeded
    );
    assert_eq!(
        flood
            .write_input(&vec![b'x'; MAX_TERMINAL_INPUT_BYTES + 1])
            .expect_err("oversized input rejected")
            .code(),
        ClaudeTerminalErrorCode::InputLimitExceeded
    );
}

#[test]
fn drop_cleans_the_fixture_process_tree() {
    let fixture = fixture();
    let profile = profile(fixture);
    let sentinel = fixture.app_data.join(format!(
        "claude-grandchild-survived-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let session = super::terminal::with_terminal_fixture("process_tree", Some(&sentinel), || {
        start_terminal_session(
            &fixture.selection,
            &profile,
            ClaudeTerminalLaunchSpec::new(ClaudeTerminalMode::Fixture, &fixture.app_data, 80, 24),
        )
        .expect("process-tree fixture")
    });
    let _ = read_until(&session, b"grandchild-started");
    drop(session);
    thread::sleep(Duration::from_secs(3));
    assert!(!sentinel.exists());
}

#[test]
fn missing_managed_binary_never_falls_back_to_an_installed_cli() {
    let isolated = build_fixture();
    let profile = profile(&isolated);
    fs::remove_file(&isolated.executable).expect("remove isolated managed binary");
    let error = start_terminal_session(
        &isolated.selection,
        &profile,
        ClaudeTerminalLaunchSpec::new(ClaudeTerminalMode::Onboarding, &isolated.app_data, 80, 24),
    )
    .expect_err("missing managed binary rejected");
    assert_eq!(error.code(), ClaudeTerminalErrorCode::InvalidSelection);
}

#[test]
fn account_profile_and_package_mismatch_is_refused() {
    let fixture = fixture();
    let codex_account = OpaqueAgentAccountRef::parse(&format!(
        "account_codex_fixture_{}",
        uuid::Uuid::new_v4().simple()
    ))
    .expect("codex account");
    let codex_binding = RuntimeProfileBinding::new(
        &codex_account,
        AgentProductId::ChatgptCodex,
        ManagedRuntimeId::CodexPythonSdk,
        "0.147.0",
    )
    .expect("codex binding");
    let wrong_profile = fixture
        .profile_store
        .create(&codex_binding)
        .expect("wrong profile");
    let error = start_terminal_session(
        &fixture.selection,
        &wrong_profile,
        ClaudeTerminalLaunchSpec::new(ClaudeTerminalMode::Onboarding, &fixture.app_data, 80, 24),
    )
    .expect_err("cross-product profile rejected");
    assert_eq!(error.code(), ClaudeTerminalErrorCode::InvalidSelection);
}

#[test]
fn claude_managed_fixture_child() {
    if let Ok(mode) = std::env::var("ALFRED_CLAUDE_STATUS_FIXTURE") {
        match mode.as_str() {
            "status_subscription" => print_status(false, true),
            "status_api_key" => print_status(true, true),
            "status_logged_out" => print_status(false, false),
            _ => std::process::exit(91),
        }
    }
    let Ok(mode) = std::env::var("ALFRED_CLAUDE_PTY_FIXTURE") else {
        return;
    };
    match mode.as_str() {
        "already_logged_in" => {
            println!("Claude Code ready");
            std::io::stdout()
                .flush()
                .expect("flush interactive fixture");
            thread::sleep(Duration::from_secs(30));
        }
        "first_login" => {
            println!("Choose login method");
            std::io::stdout().flush().expect("flush login fixture");
            let mut input = [0u8; 16];
            let read = std::io::stdin().read(&mut input).expect("login input");
            if input[..read].starts_with(b"1") {
                println!("provider-owned-browser-flow");
                std::io::stdout().flush().expect("flush login response");
            }
            thread::sleep(Duration::from_secs(30));
        }
        "logout" => {
            println!("Logged out");
        }
        "crash" => std::process::exit(17),
        "flood" => {
            let chunk = vec![b'x'; MAX_TERMINAL_OUTPUT_CHUNK_BYTES];
            for _ in 0..(MAX_BUFFERED_TERMINAL_OUTPUT_BYTES / chunk.len() + 32) {
                std::io::stdout().write_all(&chunk).expect("fixture flood");
            }
            std::io::stdout().flush().expect("flush fixture flood");
            thread::sleep(Duration::from_secs(30));
        }
        "process_tree" => {
            let sentinel = std::env::var_os("ALFRED_CLAUDE_PTY_SENTINEL")
                .map(PathBuf::from)
                .expect("process tree sentinel");
            Command::new(std::env::current_exe().expect("fixture executable"))
                .args([
                    "--exact",
                    "agents::native::providers::claude::subscription_tests::claude_managed_fixture_child",
                    "--nocapture",
                ])
                .env("ALFRED_CLAUDE_PTY_FIXTURE", "grandchild")
                .env("ALFRED_CLAUDE_PTY_SENTINEL", &sentinel)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn fixture grandchild");
            println!("grandchild-started");
            std::io::stdout()
                .flush()
                .expect("flush process tree fixture");
            thread::sleep(Duration::from_secs(30));
        }
        "grandchild" => {
            let sentinel = std::env::var_os("ALFRED_CLAUDE_PTY_SENTINEL")
                .map(PathBuf::from)
                .expect("grandchild sentinel");
            thread::sleep(Duration::from_secs(2));
            fs::write(sentinel, b"survived").expect("write survival sentinel");
        }
        _ => std::process::exit(92),
    }
}

fn print_status(environment_api_key: bool, logged_in: bool) -> ! {
    println!("{{");
    println!("  \"loggedIn\": {logged_in},");
    println!(
        "  \"authMethod\": \"{}\",",
        if logged_in { "claude.ai" } else { "none" }
    );
    println!("  \"apiProvider\": \"firstParty\",");
    println!(
        "  \"apiKeySource\": \"{}\",",
        if environment_api_key {
            "ANTHROPIC_API_KEY"
        } else if logged_in {
            "/login managed key"
        } else {
            "none"
        }
    );
    println!("  \"subscriptionType\": \"max\"");
    println!("}}");
    std::io::stdout().flush().expect("flush status fixture");
    thread::sleep(Duration::from_millis(100));
    std::process::exit(if logged_in { 0 } else { 1 });
}
