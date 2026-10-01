use super::count_code_lines;

#[test]
fn ignores_blanks_line_comments_doc_comments_and_nested_block_comments() {
    let source = "\n // comment\r\n/// docs\n//! docs\n/* outer\n /* inner */\n*/\nlet a = 1; // trailing\n/* prefix */ let b = 2; /* suffix */";
    assert_eq!(count_code_lines(source, true).unwrap(), 2);
}

#[test]
fn ignores_normal_byte_c_and_raw_strings_but_counts_surrounding_code() {
    let source = r###"
"only a string"
b"bytes"
c"c string"
r"raw"
br#"raw bytes"#
cr##"raw c"##
let text = "first
// not a comment
escaped \" quote
last";
let raw = r##"first
"# /* not a comment */
last"##;
"###;
    assert_eq!(count_code_lines(source, true).unwrap(), 4);
}

#[test]
fn lifetimes_labels_raw_identifiers_and_character_literals_remain_code() {
    let source = r###"fn f<'a>(value: &'a str) {
'label: loop { break 'label; }
let r#type = '"';
let slash = '/';
let quote = '\'';
let escaped = '\u{1F600}';
let byte = b'\x22';
let unicode = '字';
}
"###;
    assert_eq!(count_code_lines(source, true).unwrap(), 9);
}

#[test]
fn strings_do_not_open_comments_and_comments_do_not_open_strings() {
    let source = "\"/*\"\n// \"unterminated\n/* \"unterminated */\ncode();\n\"//\"";
    assert_eq!(count_code_lines(source, true).unwrap(), 1);
}

#[test]
fn cpp_strings_raw_delimiters_characters_and_continued_comments() {
    let source = r###"
u8"text"
L"wide"
u8R"tag(first
" // string contents
last)tag"
const char* text = R"(first
last)";
char quote = '"';
// continued \
still a comment
int number = 1;
auto separated = 1'000 + 0xAB'CD;
"###;
    assert_eq!(count_code_lines(source, false).unwrap(), 5);
}

#[test]
fn incomplete_comments_and_strings_fail_instead_of_hiding_remaining_code() {
    for source in ["/*", "\"", "r##\"unfinished"] {
        assert!(count_code_lines(source, true).is_err(), "{source}");
    }
    assert!(count_code_lines("R\"tag(unfinished", false).is_err());
}

#[test]
fn counts_each_line_once_and_handles_escaped_backslashes_and_crlf() {
    let source = "let a = \"\\\\\"; let b = 2;\r\n\r\n\"ignored\"\r\nlet c = 3;";
    assert_eq!(count_code_lines(source, true).unwrap(), 2);
}
