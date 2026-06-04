use super::*;

fn raios_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn detect_rust_project() {
    assert_eq!(detect_type(&raios_root()), ProjectType::Rust);
}

#[test]
fn detect_unknown_on_temp() {
    let tmp = std::env::temp_dir().join("raios_build_test_unknown");
    let _ = std::fs::create_dir_all(&tmp);
    assert_eq!(detect_type(&tmp), ProjectType::Unknown);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn parse_rust_test_output_parses_result_line() {
    let output = "test result: ok. 22 passed; 0 failed; 1 ignored; 0 measured";
    let (p, f, i, _) = rust::parse_rust_test_output(output);
    assert_eq!(p, 22);
    assert_eq!(f, 0);
    assert_eq!(i, 1);
}

#[test]
fn parse_rust_test_output_sums_multiple_binaries() {
    let output = "test result: ok. 143 passed; 3 failed; 0 ignored; 0 measured\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured";
    let (p, f, i, _) = rust::parse_rust_test_output(output);
    assert_eq!(p, 143);
    assert_eq!(f, 3);
    assert_eq!(i, 1);
}

#[test]
fn parse_jest_output_extracts_counts() {
    let output = "Tests:  47 passed, 2 failed, 49 total";
    let (p, f) = node::parse_jest_output(output);
    assert_eq!(p, 47);
    assert_eq!(f, 2);
}

#[test]
fn parse_pytest_output_extracts_counts() {
    let output = "collected 50 items\n\n47 passed, 3 failed in 1.23s";
    let (p, f) = python::parse_pytest_output(output);
    assert_eq!(p, 47);
    assert_eq!(f, 3);
}

#[test]
fn extract_num_works() {
    assert_eq!(common::extract_num("22 passed", "passed"), Some(22));
    assert_eq!(common::extract_num("no number here", "passed"), None);
}

#[test]
fn detect_android_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Android);
}

#[test]
fn detect_android_with_bat_and_settings() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew.bat")).unwrap();
    std::fs::File::create(tmp.path().join("settings.gradle")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Android);
}

#[test]
fn rust_takes_priority_over_android() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Rust);
}

#[test]
fn gradlew_alone_does_not_detect_android() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Unknown);
}

#[test]
fn detect_flutter_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\nversion: 1.0.0+1\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn flutter_takes_priority_over_ios_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("ios/Runner.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn node_takes_priority_over_flutter() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), "{\"name\":\"x\"}").unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Node);
}

#[test]
fn parse_flutter_build_success() {
    let output = "Running Gradle task 'assembleRelease'...\nBuilt build/app/outputs/apk/release/app-release.apk (7.4MB)";
    let (ok, errors) = flutter::parse_flutter_build_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_flutter_build_failure() {
    let output = "Error: A JDK was not found.\nFailed to execute gradle";
    let (ok, errors) = flutter::parse_flutter_build_output(output);
    assert!(!ok);
    assert!(errors >= 1);
}

#[test]
fn parse_flutter_test_all_pass() {
    let output = "00:03 +42: All tests passed!\n";
    let (passed, failed) = flutter::parse_flutter_test_output(output);
    assert_eq!(passed, 42);
    assert_eq!(failed, 0);
}

#[test]
fn parse_flutter_test_partial_failure() {
    let output = "00:05 +38 -3: Some tests failed.\n";
    let (passed, failed) = flutter::parse_flutter_test_output(output);
    assert_eq!(passed, 38);
    assert_eq!(failed, 3);
}

#[test]
fn parse_flutter_test_empty_output() {
    let (passed, failed) = flutter::parse_flutter_test_output("");
    assert_eq!(passed, 0);
    assert_eq!(failed, 0);
}

#[test]
fn detect_ios_xcodeproj_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("MyApp.xcodeproj")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn detect_ios_xcworkspace_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("MyApp.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn detect_ios_package_swift() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Package.swift"), "// swift-tools-version:5.9\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn flutter_takes_priority_over_xcworkspace() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("MyApp.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn parse_xcodebuild_build_success() {
    let output = "** BUILD SUCCEEDED **\n\nBuild settings from command line:";
    let (ok, errors) = ios::parse_xcodebuild_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_xcodebuild_build_failure_counts_errors() {
    let output = "/path/File.swift:10:5: error: use of undeclared type 'Foo'\n/path/File.swift:20:1: error: expected expression\n** BUILD FAILED **";
    let (ok, errors) = ios::parse_xcodebuild_output(output);
    assert!(!ok);
    assert_eq!(errors, 2);
}

#[test]
fn parse_xcodebuild_counts_warnings() {
    let output = "/path/File.swift:5:3: warning: result of call is unused\n** BUILD SUCCEEDED **";
    let warnings = ios::parse_xcodebuild_warnings(output);
    assert_eq!(warnings, 1);
}

#[test]
fn parse_xcodebuild_test_pass() {
    let output = "Test Case '-[MyTests testExample]' passed (0.001 seconds).\nTest Suite 'All tests' passed at 2026-01-01.\n** TEST SUCCEEDED **";
    let (passed, failed) = ios::parse_xcodebuild_test_output(output);
    assert_eq!(passed, 1);
    assert_eq!(failed, 0);
}

#[test]
fn parse_xcodebuild_test_mixed() {
    let output = "Test Case '-[MyTests testA]' passed (0.001 seconds).\nTest Case '-[MyTests testB]' failed (0.002 seconds).\n** TEST FAILED **";
    let (passed, failed) = ios::parse_xcodebuild_test_output(output);
    assert_eq!(passed, 1);
    assert_eq!(failed, 1);
}

#[test]
fn build_gradle_alone_does_not_detect_android() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Unknown);
}

#[test]
fn parse_gradle_build_success() {
    let output = "BUILD SUCCESSFUL in 15s\n5 actionable tasks: 5 executed";
    let (ok, errors) = android::parse_gradle_build_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_gradle_build_failure_counts_errors() {
    let output = "e: file.kt: (42, 5): error: unresolved reference: Foo\ne: file.kt: (50, 3): error: type mismatch\nBUILD FAILED in 8s";
    let (ok, errors) = android::parse_gradle_build_output(output);
    assert!(!ok);
    assert_eq!(errors, 2);
}

#[test]
fn parse_gradle_build_error_prefix_only() {
    let output = "error: Could not find or load main class GradleWrapperMain\nBUILD FAILED";
    let (ok, errors) = android::parse_gradle_build_output(output);
    assert!(!ok);
    assert_eq!(errors, 1);
}

#[test]
fn parse_gradle_build_no_false_positive_on_log_line() {
    let output = "NOTE: The Kotlin options have changed: error: is now deprecated\nBUILD SUCCESSFUL";
    let (ok, errors) = android::parse_gradle_build_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_gradle_test_output_success() {
    let output = "Tests run: 47, Failures: 2, Errors: 0, Skipped: 1\nBUILD SUCCESSFUL in 12s";
    let (passed, failed) = android::parse_gradle_test_output(output);
    assert_eq!(passed, 45);
    assert_eq!(failed, 2);
}

#[test]
fn parse_gradle_test_all_pass() {
    let output = "Tests run: 20, Failures: 0, Errors: 0, Skipped: 0\nBUILD SUCCESSFUL";
    let (passed, failed) = android::parse_gradle_test_output(output);
    assert_eq!(passed, 20);
    assert_eq!(failed, 0);
}

#[test]
fn parse_gradle_test_build_failed_no_results() {
    let output = "BUILD FAILED in 5s\nCould not connect to emulator";
    let (passed, failed) = android::parse_gradle_test_output(output);
    assert_eq!(passed, 0);
    assert_eq!(failed, 0);
}

#[test]
fn detect_esp_idf_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("idf.py")).unwrap();
    std::fs::File::create(tmp.path().join("CMakeLists.txt")).unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Embedded);
}

#[test]
fn detect_platformio_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("platformio.ini"),
        "[env:esp32dev]\nplatform = espressif32\n",
    )
    .unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Embedded);
}

#[test]
fn detect_arduino_ino_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mysketch.ino"), "void setup() {}\nvoid loop() {}\n").unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Embedded);
}

#[test]
fn android_takes_priority_over_embedded() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    std::fs::File::create(tmp.path().join("idf.py")).unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Android);
}

#[test]
fn detect_terraform_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("main.tf"),
        "terraform {\n  required_version = \">= 1.5\"\n}\n",
    )
    .unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Iac);
}

#[test]
fn detect_docker_compose_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("docker-compose.yml"),
        "version: \"3.8\"\nservices:\n  app:\n    image: nginx\n",
    )
    .unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Iac);
}

#[test]
fn detect_dockerfile_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu:22.04\n").unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Iac);
}

#[test]
fn embedded_takes_priority_over_dockerfile() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("platformio.ini"), "[env:esp32dev]\n").unwrap();
    std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu\n").unwrap();
    assert_eq!(super::detect_type(tmp.path()), super::ProjectType::Embedded);
}
