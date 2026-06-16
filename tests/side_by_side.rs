//! Regression tests for side-by-side rendering.
//!
//! Each test exercises a specific rendering defect found during review.
//! They all use real Python source through `diff_sources` so the full
//! pipeline (tree-sitter → matcher → actions → renderer) is exercised.

use std::collections::HashSet;

use diffame::languages;
use diffame::output::strip_ansi;
use diffame::output::FormatInput;
use diffame::side_by_side::format_side_by_side;
use diffame::{diff_sources, DiffOptions};

/// Helper: diff two Python snippets and return the side-by-side output with
/// ANSI colors stripped.
///
/// The formatter is a pure function that always emits ANSI styling, so the
/// "plain" and "colored" variants differ only in whether the styling is
/// stripped afterwards — no global state, safe under parallel test threads.
fn side_by_side_plain(old: &str, new: &str) -> String {
    strip_ansi(&side_by_side_colored(old, new))
}

/// Helper: diff two Python snippets and return the side-by-side output with
/// ANSI color codes included.
fn side_by_side_colored(old: &str, new: &str) -> String {
    side_by_side_lang_colored(old, new, "py")
}

/// Diffs two snippets in the given language and returns the styled
/// side-by-side output (the formatter always emits ANSI).
fn side_by_side_lang_colored(old: &str, new: &str, ext: &str) -> String {
    let profile = languages::profile_for_ext(ext).expect("language profile");
    let result = diff_sources(
        old.as_bytes(),
        new.as_bytes(),
        profile,
        &DiffOptions::default(),
    )
    .expect("diff failed");
    format_side_by_side(&FormatInput {
        source_bytes: old.as_bytes(),
        destination_bytes: new.as_bytes(),
        result: &result,
        source_filename: None,
        destination_filename: None,
        language_name: None,
        terminal_width: None,
        first_file: true,
        last_file: true,
    })
}

/// Parse plain-text rows into (left_num, left_text, right_num, right_text).
fn parse_rows(output: &str) -> Vec<(Option<usize>, String, Option<usize>, String)> {
    output
        .lines()
        .filter(|line| line.contains('│'))
        .map(|line| {
            let clean = strip_ansi(line);
            let parts: Vec<&str> = clean.split('│').collect();
            if parts.len() < 4 {
                return (None, String::new(), None, String::new());
            }
            let left_num = parts[0].trim().parse::<usize>().ok();
            let left_text = parts[1].trim().to_string();
            let right_num = parts[2].trim().parse::<usize>().ok();
            let right_text = parts[3].trim().to_string();
            (left_num, left_text, right_num, right_text)
        })
        .collect()
}

const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_CYAN: &str = "\x1b[36m";

const FUNC_OLD: &str = "\
def greet(name):
    print(\"Hello, \" + name)

def add(a, b):
    return a + b

def unused():
    pass
";

const FUNC_NEW: &str = "\
def add(a, b):
    return a + b

def greet(person):
    print(\"Hi, \" + person)

def multiply(a, b):
    return a * b

def helper():
    x = 1
    return x
";

/// Issue 1: A blank line that exists in both files at the boundary between
/// functions should not appear as a deleted row on the left with no right
/// counterpart.
#[test]
fn blank_lines_preserved_in_both_files_are_not_shown_as_deleted() {
    let output = side_by_side_plain(FUNC_OLD, FUNC_NEW);
    let rows = parse_rows(&output);

    let phantom_blank_deletes: Vec<_> = rows
        .iter()
        .filter(|(left_num, left_text, right_num, _)| {
            left_num.is_some() && left_text.trim().is_empty() && right_num.is_none()
        })
        .collect();

    assert!(
        phantom_blank_deletes.is_empty(),
        "Blank lines that exist in both files should not appear as deleted rows.\n\
         Found {} phantom blank-line delete(s): {:?}",
        phantom_blank_deletes.len(),
        phantom_blank_deletes,
    );
}

/// Issue 2: When only a single token on a line changes (e.g. `name` → `person`),
/// the unchanged portions of the line (like `def greet(`) must NOT be colored
/// yellow. Only the changed token should be yellow.
#[test]
fn updated_line_colors_only_changed_tokens() {
    let old = "def greet(name):\n    pass\n";
    let new = "def greet(person):\n    pass\n";
    let output = side_by_side_colored(old, new);

    // Find the line containing the greet definition.
    let greet_line = output
        .lines()
        .find(|line| strip_ansi(line).contains("def greet("))
        .expect("should have a line containing 'def greet('");

    // The unchanged prefix `def greet(` must NOT be wrapped in yellow.
    // If the entire line is yellow, `def greet(` will be preceded by the
    // yellow ANSI code. Check that `def greet(` does NOT appear immediately
    // after a yellow escape.
    //
    // Split by the separator │ to isolate the left content column.
    let left_content = greet_line
        .split('│')
        .nth(1)
        .expect("should have left content column");

    // If `def` is inside a yellow span, the yellow escape code will appear
    // before it with no reset in between. The whole left column being yellow
    // means ANSI_YELLOW appears and then `def` follows without a reset.
    assert!(
        !left_content.contains(&format!("{}def greet(", ANSI_YELLOW)),
        "The unchanged prefix 'def greet(' should not be colored yellow.\n\
         Full left column: {:?}",
        left_content,
    );

    // But the changed token `name` SHOULD be yellow on the left.
    assert!(
        left_content.contains(ANSI_YELLOW),
        "The changed token 'name' should be colored yellow.\n\
         Full left column: {:?}",
        left_content,
    );
}

/// Issue 3: A function that kept its relative position among its peers should
/// NOT have yellow (moved) line numbers. `unused` stayed after `add` in both
/// files, it was not reordered.
#[test]
fn non_reordered_function_is_not_marked_as_moved() {
    let output = side_by_side_colored(FUNC_OLD, FUNC_NEW);

    // Find the line where source shows "def unused".
    let unused_line = output
        .lines()
        .find(|line| strip_ansi(line).contains("def unused"))
        .expect("should have a line for def unused");

    // The source line number (left column) for this row should NOT be yellow.
    let left_number_column = unused_line
        .split('│')
        .next()
        .expect("should have left number column");

    assert!(
        !left_number_column.contains(ANSI_YELLOW),
        "Line number for 'def unused' should not be yellow (it was not moved).\n\
         Left number column: {:?}",
        left_number_column,
    );
}

/// The user's actual test case: FUNC_OLD + think appended, FUNC_NEW + think
/// appended. The output should pair greet↔greet, add↔add, think↔think because
/// the leaf-level identifiers map correctly (greet→greet, think→think).
///
/// The bug: parent-level function_definition mappings can disagree with leaf
/// mappings (greet's function_definition maps to think's function_definition),
/// and the section-constrained approach used the WRONG level to constrain,
/// garbling the output.
#[test]
fn extended_test_case_pairs_functions_by_leaf_identity() {
    let old = "\
def greet(name):
    print(\"Hello, \" + name)

def add(a, b):
    return a + b

def unused():
    pass

def think(about):
    print(about)
";
    let new = "\
def add(a, b):
    return a + b

def greet(person):
    print(\"Hi, \" + person)

def multiply(a, b):
    return a * b

def helper():
    x = 1
    return x

def think(thought):
    print(\"Thinking about \" + thought)
";
    let output = side_by_side_plain(old, new);
    let rows = parse_rows(&output);

    // No source line number may appear more than once.
    let source_line_numbers: Vec<usize> = rows
        .iter()
        .filter_map(|(left_num, _, _, _)| *left_num)
        .collect();
    let unique_source: HashSet<usize> = source_line_numbers.iter().copied().collect();
    assert_eq!(
        source_line_numbers.len(),
        unique_source.len(),
        "Source line numbers must be unique. Duplicates found in: {:?}",
        source_line_numbers,
    );

    // greet must pair with greet.
    let greet_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("def greet"))
        .expect("should have a row with 'def greet' on the right");
    assert!(
        greet_row.1.contains("def greet"),
        "right 'def greet' should pair with left 'def greet', got left={:?}",
        greet_row.1,
    );

    // think must pair with think.
    let think_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("def think"))
        .expect("should have a row with 'def think' on the right");
    assert!(
        think_row.1.contains("def think"),
        "right 'def think' should pair with left 'def think', got left={:?}",
        think_row.1,
    );

    // add must pair with add.
    let add_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("def add"))
        .expect("should have a row with 'def add' on the right");
    assert!(
        add_row.1.contains("def add"),
        "right 'def add' should pair with left 'def add', got left={:?}",
        add_row.1,
    );

    // think's print(about) must pair with think's print(... thought).
    let think_print_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("Thinking about"))
        .expect("should have a row for think's print");
    assert!(
        think_print_row.1.contains("print(about)"),
        "think's print should pair with 'print(about)', got left={:?}",
        think_print_row.1,
    );
}

/// Structural invariants that must hold for any diff output.
#[test]
fn cross_function_leaf_mapping_does_not_garble_lines() {
    let old = "\
def format_name(first, last):
    full = first + \" \" + last
    return full.upper()

def add(a, b):
    return a + b

def process(items):
    for item in items:
        print(item)
    return len(items)
";
    let new = "\
def format_name(given, family):
    full = given + \" \" + family
    return full.upper()

def add(x, y):
    return x + y

def process(entries):
    for entry in entries:
        print(entry)
    return len(entries)
";
    let output = side_by_side_plain(old, new);
    let rows = parse_rows(&output);

    // No source line number appears more than once.
    let source_line_numbers: Vec<usize> = rows
        .iter()
        .filter_map(|(left_num, _, _, _)| *left_num)
        .collect();
    let unique_source: HashSet<usize> = source_line_numbers.iter().copied().collect();
    assert_eq!(
        source_line_numbers.len(),
        unique_source.len(),
        "Source line numbers must be unique. Duplicates found in: {:?}",
        source_line_numbers,
    );

    // No destination line number appears more than once.
    let destination_line_numbers: Vec<usize> = rows
        .iter()
        .filter_map(|(_, _, right_num, _)| *right_num)
        .collect();
    let unique_destination: HashSet<usize> = destination_line_numbers.iter().copied().collect();
    assert_eq!(
        destination_line_numbers.len(),
        unique_destination.len(),
        "Destination line numbers must be unique. Duplicates found in: {:?}",
        destination_line_numbers,
    );
}

// ── Helpers for language-generic tests ──────────────────────────────────

const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";

fn side_by_side_lang_plain(old: &str, new: &str, ext: &str) -> String {
    strip_ansi(&side_by_side_lang_colored(old, new, ext))
}

/// Returns `true` if `needle` appears inside an active `color_code` span.
fn has_color_at(text: &str, needle: &str, color_code: &str) -> bool {
    let Some(pos) = text.find(needle) else {
        return false;
    };
    let before = &text[..pos];
    let mut last_esc: Option<usize> = None;
    let mut s = 0;
    while let Some(idx) = before[s..].find("\x1b[") {
        last_esc = Some(s + idx);
        s += idx + 1;
    }
    last_esc.is_some_and(|ep| before[ep..].starts_with(color_code))
}

// ── Bug-fix tests ──────────────────────────────────────────────────────

/// Bug 2: Adding a single line to a TOML dependency list must not unpair
/// every existing dependency.
#[test]
fn toml_single_addition_pairs_unchanged_lines() {
    let old = "[dependencies]\ncolored = \"2\"\nserde = \"1\"\n";
    let new = "[dependencies]\ncolored = \"2\"\nserde = \"1\"\nterminal_size = \"0.4\"\n";
    let output = side_by_side_lang_plain(old, new, "toml");
    let rows = parse_rows(&output);

    let paired = rows
        .iter()
        .filter(|(l, _, r, _)| l.is_some() && r.is_some())
        .count();
    assert!(
        paired >= 3,
        "Expected at least 3 paired rows for the unchanged lines, got {paired}.\nRows: {rows:?}",
    );
}

/// Bug 4: Inserting a line must not break pairing of its neighbours.
#[test]
fn inserted_line_does_not_break_neighbor_pairing() {
    let old = "impl Foo {\n    pub fn new() -> Self {\n        Self::default()\n    }\n}\n";
    let new =
        "impl Foo {\n    #[must_use]\n    pub fn new() -> Self {\n        Self::default()\n    }\n}\n";
    let output = side_by_side_lang_plain(old, new, "rs");
    let rows = parse_rows(&output);

    let new_fn_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("pub fn new"))
        .expect("should have 'pub fn new' on the right");
    assert!(
        new_fn_row.1.contains("pub fn new"),
        "'pub fn new' right should pair with left, got left={:?}",
        new_fn_row.1,
    );

    let must_use_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains("must_use"))
        .expect("should have '#[must_use]' on the right");
    assert!(
        must_use_row.0.is_none(),
        "'#[must_use]' should be destination-only, got left_num={:?}",
        must_use_row.0,
    );
}

/// Bug 1: Unpaired new lines must have their content highlighted green.
#[test]
fn unpaired_new_lines_are_fully_highlighted() {
    let old = "\
fn compute() -> usize {
    let default = 50;
    let content_width = default;
    content_width
}
";
    let new = "\
fn compute() -> usize {
    let content_width = {
        let default = 50;
        let chrome = 2 + 9;
        if chrome > 0 { default - chrome } else { default }
    };
    content_width
}
";
    let output = side_by_side_lang_colored(old, new, "rs");

    // The `if chrome > 0 ...` line is completely new (no old counterpart).
    // Even though `default` is mapped to a node in the old file, the entire
    // unpaired line should be green.
    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("if chrome > 0") {
            continue;
        }
        let parts: Vec<&str> = clean.split('│').collect();
        if parts.len() >= 4 && !parts[0].trim().is_empty() {
            continue;
        }
        let right_content = line.split('│').nth(3).unwrap_or("");
        assert!(
            has_color_at(right_content, "default", ANSI_GREEN),
            "Token 'default' on unpaired new line should be green.\n\
             Right content: {right_content:?}",
        );
        return;
    }
    panic!("did not find an unpaired line containing 'if chrome > 0'");
}

/// Paired lines with identical text must NOT be colored, even when the
/// AST matcher fails to map their internal nodes (e.g. on moved code).
#[test]
fn identical_paired_lines_have_no_coloring() {
    // `greet` moves from first to third position; its body lines are
    // identical but the AST mapper may fail to link interior nodes.
    let old = "\
def greet(name):
    print(\"Hello, \" + name)

def add(a, b):
    return a + b
";
    let new = "\
def add(a, b):
    return a + b

def greet(name):
    print(\"Hello, \" + name)
";
    let output = side_by_side_colored(old, new);

    // `print("Hello, " + name)` appears on both sides — the text is
    // identical. No red or green should appear in that row.
    // (If the line isn't shown at all — no nearby changes — that's fine.)
    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("Hello") {
            continue;
        }
        let left_content = line.split('│').nth(1).unwrap_or("");
        let right_content = line.split('│').nth(3).unwrap_or("");
        assert!(
            !left_content.contains(ANSI_RED) && !left_content.contains(ANSI_GREEN),
            "Identical left line should have no red/green.\nLeft: {left_content:?}",
        );
        assert!(
            !right_content.contains(ANSI_RED) && !right_content.contains(ANSI_GREEN),
            "Identical right line should have no red/green.\nRight: {right_content:?}",
        );
        return;
    }
    // Line not in output means no hunks reached it — no coloring issue.
}

/// On a paired but changed line, gap text (keywords, brackets, semicolons)
/// that exists on BOTH sides must NOT be colored.
#[test]
fn gap_text_on_changed_line_is_not_colored() {
    let old = "\
fn foo() {
    let mut v: Vec<usize> = items.to_vec();
    v
}
";
    let new = "\
fn foo() {
    let mut v: Vec<usize> = items.clone();
    v
}
";
    let output = side_by_side_lang_colored(old, new, "rs");

    // The line changed (.to_vec → .clone) but `let`, `<`, `>`, `:`, `;`
    // are identical gap text that must NOT be red/green.
    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("to_vec") {
            continue;
        }
        let left_content = line.split('│').nth(1).unwrap_or("");
        // `let` must not be red on the old side.
        assert!(
            !has_color_at(left_content, "let", ANSI_RED),
            "'let' should not be red on a changed line.\nLeft: {left_content:?}",
        );
        return;
    }
    panic!("did not find a line containing 'to_vec'");
}

/// Multi-byte Unicode characters (like box-drawing `─`) must not break
/// column alignment. The display width of `─` is 1, not its 3-byte UTF-8
/// length.
#[test]
fn unicode_box_drawing_does_not_break_layout() {
    let old = "fn sep() {\n    let s = \"─┼─\";\n    s\n}\n";
    let new = "fn sep() {\n    let s = \"A┼B\";\n    s\n}\n";
    let output = side_by_side_lang_plain(old, new, "rs");

    // Every row with a separator │ must have exactly 4 columns.
    for line in output.lines() {
        if !line.contains('│') {
            continue;
        }
        let clean = strip_ansi(line);
        let cols: Vec<&str> = clean.split('│').collect();
        assert_eq!(
            cols.len(),
            4,
            "Every row should have 4 columns (3 separators), got {}.\nLine: {:?}",
            cols.len(),
            clean,
        );
    }
}

/// Keywords and operators are tree-sitter anonymous tokens.  When one is
/// ADDED on a paired changed line it must appear green, not unstyled.
#[test]
fn added_keyword_on_changed_line_is_green() {
    let old = "\
fn foo() {
    bar();
}
";
    let new = "\
fn foo() {
    let x = bar();
    x
}
";
    let output = side_by_side_lang_colored(old, new, "rs");

    // `bar();` paired with `let x = bar();`.  `let` is a new keyword.
    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("let x = bar") {
            continue;
        }
        let right_content = line.split('│').nth(3).unwrap_or("");
        assert!(
            has_color_at(right_content, "let", ANSI_GREEN),
            "'let' added to a changed line should be green.\nRight: {right_content:?}",
        );
        return;
    }
    panic!("did not find a line containing 'let x = bar'");
}

/// When a keyword is REMOVED on a paired changed line it must appear red.
#[test]
fn removed_keyword_on_changed_line_is_red() {
    let old = "\
fn foo() {
    if flag { bar(); }
}
";
    let new = "\
fn foo() {
    bar();
}
";
    let output = side_by_side_lang_colored(old, new, "rs");

    // `if flag { bar(); }` paired with `bar();`.  `if` is removed.
    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("if flag") {
            continue;
        }
        let left_content = line.split('│').nth(1).unwrap_or("");
        assert!(
            has_color_at(left_content, "if", ANSI_RED),
            "'if' removed from a changed line should be red.\nLeft: {left_content:?}",
        );
        return;
    }
    panic!("did not find a line containing 'if flag'");
}

/// When functions are reordered, identical lines like `return a + b`
/// that fall between non-monotonic anchors must still be paired.
#[test]
fn reordered_functions_still_pair_identical_lines() {
    let output = side_by_side_plain(FUNC_OLD, FUNC_NEW);
    let rows = parse_rows(&output);

    // `return a + b` exists in both files — it must be paired.
    let return_row = rows
        .iter()
        .find(|(_, left, _, _)| left.contains("return a + b"))
        .expect("should have 'return a + b' on the left");
    assert!(
        return_row.2.is_some() && return_row.3.contains("return a + b"),
        "'return a + b' on left must pair with identical right, got right={:?}",
        return_row.3,
    );
}

/// A comment that replaces a code line must be fully green — both the
/// `//` prefix AND the comment text.  tree-sitter splits `//` as a
/// separate child of `line_comment`; the comment body must not become
/// invisible gap text.
#[test]
fn added_comment_is_fully_green() {
    let old = "\
fn foo() {
    old_call();
    bar();
}
";
    let new = "\
fn foo() {
    // this is new
    bar();
}
";
    let output = side_by_side_lang_colored(old, new, "rs");

    for line in output.lines() {
        let clean = strip_ansi(line);
        if !clean.contains("this is new") {
            continue;
        }
        let right_content = line.split('│').nth(3).unwrap_or("");
        // The whole comment text (not just //) must be green.
        assert!(
            has_color_at(right_content, "this is new", ANSI_GREEN),
            "Comment body should be green, not just the '//' prefix.\n\
             Right: {right_content:?}",
        );
        return;
    }
    panic!("did not find a line containing 'this is new'");
}

/// When a statement-style function body is refactored to expression style,
/// `.collect();` must pair with `.collect()` (nearly identical), and the
/// now-removed `result`-style return line must be left unpaired — not
/// paired positionally with a completely dissimilar line.
#[test]
fn similar_lines_pair_instead_of_positional_neighbors() {
    let old = "\
fn kept(node: &Node, profile: &Profile) -> Vec<Node> {
    let children: Vec<Node> = node
        .named_children()
        .filter(|child| profile.keep(child))
        .collect();
    children
}
";
    let new = "\
fn kept(node: &Node, profile: &Profile) -> Vec<Node> {
    node
        .children()
        .filter(|child| profile.keep(child) || child.is_leaf())
        .collect()
}
";
    let output = side_by_side_lang_plain(old, new, "rs");
    let rows = parse_rows(&output);

    // The right `.collect()` row must have `.collect` on the left too.
    let collect_row = rows
        .iter()
        .find(|(_, _, _, right)| right.contains(".collect()"))
        .expect("should have '.collect()' on the right");
    assert!(
        collect_row.1.contains(".collect"),
        "right '.collect()' should pair with left '.collect();', got left={:?}",
        collect_row.1,
    );

    // The bare `children` return line was removed — it must be left-only.
    let children_row = rows
        .iter()
        .find(|(_, left, _, _)| left.trim() == "children")
        .expect("should have the bare 'children' return line on the left");
    assert!(
        children_row.2.is_none(),
        "the removed 'children' return line must be unpaired, got right_num={:?} right={:?}",
        children_row.2,
        children_row.3,
    );
}

// ── Header and moved-line rendering ─────────────────────────────────────

const ANSI_BOLD: &str = "\x1b[1m";

/// Renders a diff of `a.py` vs `b.py` with filenames and language set,
/// returning the styled output.
fn side_by_side_with_header(old: &str, new: &str) -> String {
    let profile = languages::profile_for_ext("py").expect("python profile");
    let result = diff_sources(
        old.as_bytes(),
        new.as_bytes(),
        profile,
        &DiffOptions::default(),
    )
    .expect("diff failed");
    format_side_by_side(&FormatInput {
        source_bytes: old.as_bytes(),
        destination_bytes: new.as_bytes(),
        result: &result,
        source_filename: Some("a.py"),
        destination_filename: Some("b.py"),
        language_name: languages::language_name_for_ext("py"),
        terminal_width: None,
        first_file: true,
        last_file: true,
    })
}

/// The header shows each file's name on its own side, inside the column
/// layout, with a bold filename and a cyan language tag.
#[test]
fn header_shows_each_filename_on_its_side() {
    let output = side_by_side_with_header("x = 1\n", "x = 2\n");
    let header = output
        .lines()
        .find(|line| strip_ansi(line).contains("a.py"))
        .expect("output has a header line");
    let clean = strip_ansi(header);

    let parts: Vec<&str> = clean.split('│').collect();
    assert!(
        parts.len() >= 4,
        "header must use the column layout so separators align, got {clean:?}",
    );
    assert!(
        parts[1].contains("a.py") && !parts[1].contains("b.py"),
        "left header cell should name only the old file, got {:?}",
        parts[1],
    );
    assert!(
        parts[3].contains("b.py") && !parts[3].contains("a.py"),
        "right header cell should name only the new file, got {:?}",
        parts[3],
    );
    assert!(
        parts[1].contains("[Python]"),
        "language tag should be properly capitalized, got {:?}",
        parts[1],
    );
    assert!(
        !parts[3].contains("[Python]"),
        "language tag must appear only on the left side, got right {:?}",
        parts[3],
    );
    assert!(
        has_color_at(header, "a.py", ANSI_BOLD),
        "filename should be bold, header: {header:?}",
    );
    assert!(
        has_color_at(header, "Python", ANSI_CYAN),
        "language name should be cyan, header: {header:?}",
    );
    assert!(
        !has_color_at(header, "[", ANSI_CYAN),
        "brackets around the language name should be uncolored, header: {header:?}",
    );
}

/// When both sides name the same file (git-style diff), the filename appears
/// only on the left.  Language tag is always left-only.
#[test]
fn same_filename_is_not_repeated_on_right() {
    let profile = languages::profile_for_ext("py").expect("python profile");
    let result = diff_sources(b"x = 1\n", b"x = 2\n", profile, &DiffOptions::default())
        .expect("diff failed");
    let output = format_side_by_side(&FormatInput {
        source_bytes: b"x = 1\n",
        destination_bytes: b"x = 2\n",
        result: &result,
        source_filename: Some("app.py"),
        destination_filename: Some("app.py"),
        language_name: Some("Python"),
        terminal_width: None,
        first_file: true,
        last_file: true,
    });
    let header = output
        .lines()
        .find(|line| strip_ansi(line).contains("app.py"))
        .expect("output has a header line");
    let clean = strip_ansi(header);
    let parts: Vec<&str> = clean.split('│').collect();

    assert!(
        parts[1].contains("app.py"),
        "left cell should have filename, got {:?}",
        parts[1],
    );
    assert!(
        !parts[3].contains("app.py"),
        "right cell must not repeat the filename, got {:?}",
        parts[3],
    );
}

/// A standalone diff renders as one closed table: flat `┬` top rule, `┼`
/// rule under the header, and a `┴` bottom rule after the last hunk.
#[test]
fn standalone_table_is_framed_top_and_bottom() {
    let output = side_by_side_with_header("x = 1\n", "x = 2\n");
    let lines: Vec<String> = output.lines().map(strip_ansi).collect();
    assert!(
        lines[0].contains('┬') && !lines[0].contains('┼'),
        "first line should be a flat top rule with ┬ junctions, got {:?}",
        lines[0],
    );
    assert!(
        lines[1].contains("a.py"),
        "header row should follow the top rule, got {:?}",
        lines[1],
    );
    assert!(
        lines[2].contains('┼'),
        "a ┼ rule should separate the header from the rows, got {:?}",
        lines[2],
    );
    let last = lines.last().unwrap();
    assert!(
        last.contains('┴') && !last.contains('┼'),
        "the table must be closed by a ┴ bottom rule, got {last:?}",
    );
}

/// A file in the middle of a concatenated sequence (one git invocation per
/// path) opens with a `┼` rule that connects to the previous file's rows and
/// leaves the bottom open for the next file's rule.
#[test]
fn middle_file_connects_to_neighbours() {
    let profile = languages::profile_for_ext("py").expect("python profile");
    let result = diff_sources(b"x = 1\n", b"x = 2\n", profile, &DiffOptions::default())
        .expect("diff failed");
    let output = format_side_by_side(&FormatInput {
        source_bytes: b"x = 1\n",
        destination_bytes: b"x = 2\n",
        result: &result,
        source_filename: Some("a.py"),
        destination_filename: Some("b.py"),
        language_name: Some("Python"),
        terminal_width: None,
        first_file: false,
        last_file: false,
    });
    let lines: Vec<String> = output.lines().map(strip_ansi).collect();
    assert!(
        lines[0].contains('┼') && !lines[0].contains('┬'),
        "a middle file's top rule must connect with ┼ junctions, got {:?}",
        lines[0],
    );
    let last = lines.last().unwrap();
    assert!(
        !last.contains('┴'),
        "a middle file must not close the table, got {last:?}",
    );
}

/// A filename longer than the content column wraps onto further visual lines
/// exactly like code rows do, instead of overflowing and breaking the layout.
#[test]
fn long_filename_wraps_without_breaking_layout() {
    let long_name = "directory/".repeat(12) + "file.py"; // 127 chars > 50-column cell
    let profile = languages::profile_for_ext("py").expect("python profile");
    let result = diff_sources(b"x = 1\n", b"x = 2\n", profile, &DiffOptions::default())
        .expect("diff failed");
    let output = format_side_by_side(&FormatInput {
        source_bytes: b"x = 1\n",
        destination_bytes: b"x = 2\n",
        result: &result,
        source_filename: Some(&long_name),
        destination_filename: Some("b.py"),
        language_name: Some("Python"),
        terminal_width: None, // deterministic default: 50-column content cells
        first_file: true,
        last_file: true,
    });
    let lines: Vec<String> = output.lines().map(strip_ansi).collect();

    // Every line is either a rule or keeps the 4-column layout; the long
    // name must not push extra `│` separators out of alignment.
    for line in &lines {
        assert!(
            line.contains('┬')
                || line.contains('┼')
                || line.contains('┴')
                || line.matches('│').count() == 3,
            "every row must keep the 4-column layout, got {line:?}",
        );
    }

    // The wrapped filename is reassembled across the header's visual lines.
    let header_text: String = lines
        .iter()
        .take_while(|l| !l.contains('┼'))
        .map(|l| {
            l.split('│')
                .nth(1)
                .unwrap_or("")
                .trim_matches(' ')
                .to_string()
        })
        .collect();
    assert!(
        header_text.contains("file.py"),
        "the tail of the wrapped filename must appear, header lines: {lines:?}",
    );
    assert!(
        header_text.starts_with("[Python]"),
        "the header should start with the language tag, header lines: {lines:?}",
    );
    assert!(
        header_text.contains("directory/"),
        "the head of the wrapped filename must appear, header lines: {lines:?}",
    );
}

/// Reordered lines are flagged with yellow line numbers (yellow = changed,
/// consistent with updated tokens).
#[test]
fn moved_lines_have_yellow_numbers() {
    let old = "\
def first():
    return 1

def second():
    return 2
";
    let new = "\
def second():
    return 2

def first():
    return 1
";
    let output = side_by_side_colored(old, new);
    let moved_line = output.lines().find(|line| {
        let clean = strip_ansi(line);
        clean.contains("def ")
            && line
                .split('│')
                .next()
                .is_some_and(|n| n.contains(ANSI_YELLOW))
    });
    assert!(
        moved_line.is_some(),
        "at least one reordered 'def' line should have a yellow line number.\n\
         Output:\n{output}",
    );
}

/// When lines differ only in variable names (a rename refactor), they must
/// be paired side-by-side with the changed identifiers highlighted, not
/// shown as separate deletions and insertions.
///
/// The fixture files are real before/after snapshots of a variable-rename
/// refactor across a ~320-line Rust file. With this many simultaneous
/// renames the AST matcher cannot anchor every identifier, so lines that
/// differ only in variable names end up unanchored. The line pairing must
/// still pair them by text similarity rather than leaving them as
/// unpaired deletions and insertions.
#[test]
fn renamed_variables_produce_paired_lines_not_delete_insert() {
    let old = include_str!("fixtures/line_pairing_before.rs");
    let new = include_str!("fixtures/line_pairing_after.rs");

    let output = side_by_side_lang_colored(old, new, "rs");
    let plain = strip_ansi(&output);
    let rows = parse_rows(&plain);

    // Old line: `gaps.push((0, dst_count, 0, src_count));`
    // New line: `gaps.push((0, destination_count, 0, source_count));`
    // Only the variable names changed. It must be PAIRED.
    let push_rows: Vec<_> = rows
        .iter()
        .filter(|(_, left, _, right)| left.contains("gaps.push") || right.contains("gaps.push"))
        .collect();

    // The first gaps.push (the one inside `if anchors.is_empty()`) is the
    // clearest case: identical structure, only variable names differ.
    let first_push = push_rows
        .first()
        .unwrap_or_else(|| panic!("no row contains 'gaps.push'.\nOutput:\n{plain}"));

    assert!(
        first_push.0.is_some() && first_push.2.is_some(),
        "the first gaps.push line should be paired (both sides have line numbers), \
         not shown as a deletion and insertion.\n\
         Got: left_num={:?} right_num={:?}\nOutput:\n{plain}",
        first_push.0,
        first_push.2,
    );
}
