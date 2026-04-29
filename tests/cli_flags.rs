use std::process::Command;

#[test]
fn version_flags_print_version_without_ssh_passthrough() {
    let binary = env!("CARGO_BIN_EXE_pickey");
    let expected = format!("pickey {}\n", env!("CARGO_PKG_VERSION"));

    for flag in ["--version", "-V"] {
        let output = Command::new(binary).arg(flag).output().unwrap();

        assert!(
            output.status.success(),
            "{flag} failed with stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    }
}
