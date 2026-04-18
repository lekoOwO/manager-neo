use std::process::Command;

#[test]
fn cli_help_smoke_test() {
    let output = Command::new(env!("CARGO_BIN_EXE_manager-neo-backend"))
        .arg("--help")
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout not utf8");
    assert!(stdout.contains("manager-neo"));
    assert!(stdout.contains("instance"));
    assert!(stdout.contains("template"));
}
