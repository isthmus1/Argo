use clang::{Clang, EntityKind, Index, Linkage, Unsaved};

const EXAMPLE_C: &str = r#"
extern int add(int a, int b);
int add(int a, int b) { return a + b; }
static int helper(void) { return 1; }
"#;

/// Returns `["add: int (int, int)"]`; `helper` is `static`, so it isn't listed.
pub fn example() -> Result<Vec<String>, String> {
    let clang = Clang::new()?;
    let index = Index::new(&clang, false, false);
    let unit = index
        .parser("example.c")
        .unsaved(&[Unsaved::new("example.c", EXAMPLE_C)])
        .parse()
        .map_err(|e| e.to_string())?;

    Ok(unit
        .get_entity()
        .get_children()
        .into_iter()
        .filter(|e| e.get_kind() == EntityKind::FunctionDecl && e.is_definition())
        .filter(|e| e.get_linkage() == Some(Linkage::External))
        .filter_map(|e| {
            Some(format!(
                "{}: {}",
                e.get_name()?,
                e.get_type()?.get_display_name()
            ))
        })
        .collect())
}
