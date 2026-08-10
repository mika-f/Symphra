use std::io::Write;
use std::process::{Command, Stdio};

fn run_with_stdin(input: &str) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_symphra-formatter"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary should spawn");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(input.as_bytes())
        .expect("stdin should accept input");
    let output = child.wait_with_output().expect("process should exit");
    (
        output.status.success(),
        String::from_utf8(output.stdout).expect("stdout should be utf8"),
        String::from_utf8(output.stderr).expect("stderr should be utf8"),
    )
}

#[test]
fn formats_stdin_to_stdout() {
    let (success, stdout, _) = run_with_stdin("project { seed 1 sample_rate 8khz output mono }\n");

    assert!(success);
    assert_eq!(
        stdout,
        "project {\n  seed 1\n  sample_rate 8khz\n  output mono\n}\n"
    );
}

#[test]
fn writes_nothing_to_stdout_for_invalid_source() {
    let (success, stdout, stderr) = run_with_stdin("project {");

    assert!(!success);
    assert!(stdout.is_empty());
    assert!(!stderr.is_empty());
}
