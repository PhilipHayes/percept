//! C# in aq-treesitter (tree-sitter-c-sharp 0.23.5): language registration, and
//! the grammar facts that downstream consumers (canopy's index) depend on.
use aq_core::backend::Backend;
use aq_core::{eval, lex, parse, result_to_json};
use aq_treesitter::langs::Language;
use aq_treesitter::parse::ParsedTree;
use aq_treesitter::TreeSitterBackend;

const SOURCE: &str = r#"
using System;
using System.Threading.Tasks;

namespace PerformX.Cards.Onbe;

public delegate void Handler<T>(T item);

public interface ICardProvider
{
    Task<ProviderResult> LoadAsync(string cardId);
    string Name { get; }
}

public record ProviderResult(string Id, int Amount);
public record struct Point(int X, int Y);
public readonly struct Money { }

public enum Kind { Debit, Credit = 2 }

public abstract partial class Repo<T> : Base<T>, IDisposable where T : class
{
    public event EventHandler Changed;
    private readonly int _a, _b;
    public int Id { get; set; }
    public Repo(int a) : base(a) { }
    ~Repo() { }
    public static Repo<T> operator +(Repo<T> l, Repo<T> r) => l;
    public int this[int i] => i;
    public void Run()
    {
        void Local() { }
        var x = new Helper();
        Local();
    }
    public class Nested { }
}

namespace Legacy.Block
{
    public class Old { }
}
"#;

fn q(query: &str) -> Vec<serde_json::Value> {
    let tree =
        ParsedTree::parse(SOURCE.to_string(), Language::CSharp, Some("t.cs".into())).unwrap();
    let root = tree.to_owned_node();
    let results = eval(&parse(&lex(query).unwrap()).unwrap(), &root).unwrap();
    results.iter().map(result_to_json).collect()
}

fn names(kind: &str) -> Vec<String> {
    q(&format!("desc:{kind} | .name | @text"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn csharp_from_extension_maps_cs_and_csx() {
    assert_eq!(Language::from_extension("cs"), Some(Language::CSharp));
    assert_eq!(Language::from_extension("csx"), Some(Language::CSharp));
    assert_eq!(
        Language::from_path(std::path::Path::new("src/Cards/Program.cs")),
        Some(Language::CSharp)
    );
}

#[test]
fn csharp_from_name_accepts_csharp_and_cs() {
    for n in ["csharp", "CSharp", "c#", "cs"] {
        assert_eq!(Language::from_name(n), Some(Language::CSharp), "{n}");
    }
    assert_eq!(Language::CSharp.name(), "C#");
}

#[test]
fn csharp_in_supported_languages() {
    assert!(TreeSitterBackend.supported_languages().contains(&"csharp"));
}

#[test]
fn csharp_source_parses_without_errors() {
    let tree = ParsedTree::parse(SOURCE.to_string(), Language::CSharp, None).unwrap();
    assert!(!tree.root_node().has_error());
}

#[test]
fn csharp_type_declarations() {
    assert_eq!(names("class_declaration"), ["Repo", "Nested", "Old"]);
    assert_eq!(names("interface_declaration"), ["ICardProvider"]);
    assert_eq!(names("struct_declaration"), ["Money"]);
    assert_eq!(names("enum_declaration"), ["Kind"]);
    assert_eq!(names("delegate_declaration"), ["Handler"]);
    assert_eq!(names("enum_member_declaration"), ["Debit", "Credit"]);
}

/// `record`, `record class` and `record struct` are all `record_declaration`;
/// only an anonymous `struct`/`class` token tells them apart (the named-only
/// tree drops it, subtree text keeps it).
#[test]
fn csharp_records_share_one_kind() {
    assert_eq!(names("record_declaration"), ["ProviderResult", "Point"]);
    let texts = q("desc:record_declaration | @text");
    assert!(texts[1]
        .as_str()
        .unwrap()
        .starts_with("public record struct "));
}

#[test]
fn csharp_namespaces_block_and_file_scoped() {
    assert_eq!(
        names("file_scoped_namespace_declaration"),
        ["PerformX.Cards.Onbe"]
    );
    assert_eq!(names("namespace_declaration"), ["Legacy.Block"]);
}

#[test]
fn csharp_members() {
    assert_eq!(names("method_declaration"), ["LoadAsync", "Run"]);
    assert_eq!(names("constructor_declaration"), ["Repo"]);
    assert_eq!(names("destructor_declaration"), ["Repo"]);
    assert_eq!(names("property_declaration"), ["Name", "Id"]);
    assert_eq!(names("local_function_statement"), ["Local"]);
}

/// Fields and events carry their names one level down, in a
/// `variable_declaration > variable_declarator`; operators and indexers have no
/// `name` field at all.
#[test]
fn csharp_field_event_operator_indexer_shapes() {
    let fields: Vec<_> = q("desc:field_declaration | desc:variable_declarator | .name | @text");
    assert_eq!(fields, vec!["_a", "_b"]);
    let events: Vec<_> =
        q("desc:event_field_declaration | desc:variable_declarator | .name | @text");
    assert_eq!(events, vec!["Changed"]);
    assert_eq!(q("desc:operator_declaration | .name | @text").len(), 0);
    assert_eq!(q("desc:indexer_declaration | .name | @text").len(), 0);
    assert_eq!(q("desc:operator_declaration | @type").len(), 1);
    assert_eq!(q("desc:indexer_declaration | @type").len(), 1);
}

#[test]
fn csharp_inheritance_and_creation_shapes() {
    let bases = q("desc:class_declaration | desc:base_list | desc:identifier | @text");
    assert!(bases.contains(&serde_json::json!("IDisposable")));
    let created = q("desc:object_creation_expression | .type | @text");
    assert_eq!(created, vec![serde_json::json!("Helper")]);
}
