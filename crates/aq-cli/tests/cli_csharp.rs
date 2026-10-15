/// CLI integration tests for C#: `aq` infers the language from `.cs` / `.csx`.
use std::io::Write;
use std::process::Command;

const CSHARP: &str = r#"
namespace PerformX.Cards.Onbe;

public interface ICardProvider
{
    Task<string> LoadAsync(string cardId);
}

public sealed class OnbeCardProvider : ICardProvider
{
    public Task<string> LoadAsync(string cardId) => Task.FromResult(cardId);
}
"#;

fn run(suffix: &str, query: &str) -> std::process::Output {
    let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    f.write_all(CSHARP.as_bytes()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_aq"))
        .args(["--format", "compact", query])
        .arg(f.path())
        .output()
        .expect("failed to run aq")
}

fn names(suffix: &str, query: &str) -> Vec<String> {
    let output = run(suffix, query);
    assert!(
        output.status.success(),
        "aq failed on {suffix}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| l.trim_matches('"').to_string())
        .collect()
}

#[test]
fn aq_infers_csharp_from_cs_extension() {
    assert_eq!(
        names(".cs", "desc:class_declaration | .name | @text"),
        ["OnbeCardProvider"]
    );
    assert_eq!(
        names(".cs", "desc:interface_declaration | .name | @text"),
        ["ICardProvider"]
    );
}

#[test]
fn aq_infers_csharp_from_csx_extension() {
    assert_eq!(
        names(".csx", "desc:method_declaration | .name | @text"),
        ["LoadAsync", "LoadAsync"]
    );
}

/// AQ-2: the checker knows C#'s kinds and points a Python/Java-shaped guess at
/// the C# spelling.
#[test]
fn aq_suggests_the_csharp_kind_for_a_near_miss() {
    let output = run(".cs", "desc:class_definition | .name | @text");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("class_declaration"), "{stderr}");
}
