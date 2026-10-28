//! `--kinds function` must remove exactly the functions rustc reports as never used: never a live
//! one (the crate still builds and its tests still pass), and every dead one (none is reported
//! after), wherever it is declared and whatever surrounds it

mod common;

use std::{collections::BTreeSet, fs, path::Path};

use cargo_metadata::Message;
use common::{cargo, minify, scratch};

/// The functions rustc reports as never used in either library build
fn unused_functions(root: &Path) -> BTreeSet<String> {
    let mut unused = BTreeSet::new();
    for profile in ["dev", "test"] {
        let args = [
            "check",
            "--lib",
            "--profile",
            profile,
            "--message-format=json",
        ];
        let stdout = cargo(root, &args).stdout;
        for message in Message::parse_stream(stdout.as_slice()) {
            if let Message::CompilerMessage(message) = message.unwrap() {
                let message = message.message.message;
                if let Some(ident) = message.strip_prefix("function `") {
                    unused.insert(ident.strip_suffix("` is never used").unwrap().to_owned());
                }
            }
        }
    }
    unused
}

/// The unit tests of the library, which must all pass
fn passing_tests(root: &Path) -> String {
    cargo(root, &["test", "--lib", "--quiet"]);
    let out = cargo(root, &["test", "--lib", "--quiet", "--", "--list"]);
    String::from_utf8(out.stdout).unwrap()
}

/// Minifies `before` into exactly `after`, leaving only the `kept` functions reported as unused
fn assert_minifies(name: &str, before: &str, after: &str, kept: &[&str]) {
    let root = scratch(&format!("functions-{name}"));
    let manifest = "[package]\nname = \"p\"\nedition = \"2024\"\n";
    fs::write(root.join("Cargo.toml"), manifest).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let lib_rs = root.join("src/lib.rs");
    fs::write(&lib_rs, before).unwrap();
    let tests = passing_tests(&root);

    minify(&root, &["--kinds", "function"]);

    assert_eq!(fs::read_to_string(&lib_rs).unwrap(), after);
    assert_eq!(passing_tests(&root), tests);
    let kept = kept.iter().map(|&ident| ident.to_owned()).collect();
    assert_eq!(unused_functions(&root), kept);

    let out = minify(&root, &["--kinds", "function"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("no unused code"));
}

#[test]
fn name_shared_with_a_live_function_in_a_module() {
    assert_minifies(
        "mod",
        r#"pub fn live() -> i32 { helper() + inner::run() }
fn helper() -> i32 { 1 }
mod inner {
    fn helper() -> i32 { 2 }
    pub fn run() -> i32 { 3 }
}
"#,
        r#"pub fn live() -> i32 { helper() + inner::run() }
fn helper() -> i32 { 1 }
mod inner {
    pub fn run() -> i32 { 3 }
}
"#,
        &[],
    );
}

#[test]
fn name_shared_with_a_live_function_declared_first() {
    assert_minifies(
        "order",
        r#"mod inner {
    pub fn helper() -> i32 { 2 }
}
fn helper() -> i32 { 1 }
pub fn live() -> i32 { inner::helper() }
"#,
        r#"mod inner {
    pub fn helper() -> i32 { 2 }
}
pub fn live() -> i32 { inner::helper() }
"#,
        &[],
    );
}

#[test]
fn every_dead_function_sharing_a_name() {
    assert_minifies(
        "same-name",
        r#"mod a {
    mod b {
        fn dead() {}
        pub fn live() -> i32 { 1 }
    }
    pub fn live() -> i32 { b::live() }
    fn dead() {}
}
pub fn live() -> i32 { a::live() }
fn dead() {}
"#,
        r#"mod a {
    mod b {
        pub fn live() -> i32 { 1 }
    }
    pub fn live() -> i32 { b::live() }
}
pub fn live() -> i32 { a::live() }
"#,
        &[],
    );
}

#[test]
fn functions_nested_in_function_bodies() {
    assert_minifies(
        "nested",
        r#"fn helper() -> i32 { 1 }
pub fn live() -> i32 {
    fn helper() -> i32 { 2 }
    fn used() -> i32 { 3 }
    const INNER: i32 = 4;
    used()
}
"#,
        r#"pub fn live() -> i32 {
    fn used() -> i32 { 3 }
    const INNER: i32 = 4;
    used()
}
"#,
        &[],
    );
}

#[test]
fn transitively_dead_functions() {
    assert_minifies(
        "transitive",
        r#"fn a() -> i32 { b() }
fn b() -> i32 { c() }
fn c() -> i32 { 1 }
fn d() -> i32 { 2 }
pub fn live() -> i32 { d() }
"#,
        r#"fn d() -> i32 { 2 }
pub fn live() -> i32 { d() }
"#,
        &[],
    );
}

#[test]
fn functions_sharing_a_line() {
    assert_minifies(
        "line",
        "fn a() {} pub fn live() {} fn b() {}\nfn c() {} fn d() {}\n",
        "pub fn live() {} ",
        &[],
    );
}

#[test]
fn attributes_and_comments_go_with_their_function() {
    assert_minifies(
        "attributes",
        r#"pub fn live() {}

/// Documented
#[inline]
#[allow(clippy::all)]
fn dead() {}

// A comment of its own
pub fn live2() {}
"#,
        r#"pub fn live() {}


// A comment of its own
pub fn live2() {}
"#,
        &[],
    );
}

#[test]
fn only_the_dead_one_of_cfg_twins() {
    assert_minifies(
        "cfg",
        r#"#[cfg(unix)]
fn twin() {}
#[cfg(not(unix))]
fn twin() {}
pub fn live() {}
"#,
        r#"#[cfg(not(unix))]
fn twin() {}
pub fn live() {}
"#,
        &[],
    );
}

#[test]
fn not_a_function_only_unit_tests_use() {
    let source = r#"fn helper() -> i32 { 1 }
pub fn live() {}
#[cfg(test)]
mod tests {
    #[test]
    fn t() { assert_eq!(super::helper(), 1); }
}
"#;
    assert_minifies("unit-tests", source, source, &["helper"]);
}

#[test]
fn not_a_function_only_unit_tests_leave_dead() {
    let source = r#"fn helper() -> i32 { 1 }
#[cfg(not(test))]
pub fn live() -> i32 { helper() }
"#;
    assert_minifies("not-unit-tests", source, source, &["helper"]);
}

#[test]
fn non_ascii_text() {
    assert_minifies(
        "unicode",
        "// ü\npub fn live() -> &'static str { \"é😀\" }\nfn dead() -> &'static str { \"日本\" } fn dead2() {}\npub fn live2() {}\n",
        "// ü\npub fn live() -> &'static str { \"é😀\" }\npub fn live2() {}\n",
        &[],
    );
}

#[test]
fn crlf_line_endings() {
    let crlf = |code: &str| code.replace('\n', "\r\n");
    assert_minifies(
        "crlf",
        &crlf(
            "pub fn live() -> i32 { 1 }\nfn dead() {}\n/// ü\nfn dead2() {}\npub fn live2() {}\n",
        ),
        &crlf("pub fn live() -> i32 { 1 }\npub fn live2() {}\n"),
        &[],
    );
}

#[test]
fn foreign_functions_and_their_emptied_block() {
    assert_minifies(
        "foreign",
        r#"unsafe extern "C" {
    fn dead_ext();
}
unsafe extern "C" {
    pub safe fn abs(x: i32) -> i32;
    fn labs(x: i64) -> i64;
}
pub fn live() -> i32 { abs(-1) }
"#,
        r#"unsafe extern "C" {
    pub safe fn abs(x: i32) -> i32;
}
pub fn live() -> i32 { abs(-1) }
"#,
        &[],
    );
}

#[test]
fn every_flavor_of_function() {
    assert_minifies(
        "flavors",
        r#"unsafe extern "C" fn c() {}
extern "C-unwind" fn c_unwind() {}
const fn konst() -> i32 { 1 }
async fn not_awaited() {}
unsafe fn raw(p: *const i32) -> i32 { unsafe { *p } }
fn generic<T: Clone>(t: &T) -> T where T: Default { t.clone() }
fn r#type() {}
pub fn live() {}
"#,
        "pub fn live() {}\n",
        &[],
    );
}

#[test]
fn not_exported_or_otherwise_used_functions() {
    let source = r#"#[unsafe(no_mangle)]
extern "C" fn no_mangle() {}
#[unsafe(export_name = "exported")]
extern "C" fn export_name() {}
fn as_pointer(x: i32) -> i32 { x }
pub fn live() -> fn(i32) -> i32 { as_pointer }
#[unsafe(no_mangle)]
pub static CALLBACK: extern "C" fn() = no_mangle;
"#;
    assert_minifies("used", source, source, &[]);
}

#[test]
fn not_other_kinds_of_unused_code() {
    let source = r#"pub trait T { fn method(&self) {} }
struct S;
impl S { fn associated() {} }
const C: i32 = 1;
static S2: i32 = 2;
macro_rules! generate { () => { fn generated() {} } }
generate!();
pub fn live() {}
"#;
    assert_minifies("kinds", source, source, &["generated"]);
}

#[test]
fn live_functions_keep_their_behavior() {
    assert_minifies(
        "behavior",
        r#"fn dead() -> i32 { 0 }
fn helper(x: i32) -> i32 { x * 2 }
pub fn live(x: i32) -> i32 { helper(x) + 1 }
#[cfg(test)]
mod tests {
    #[test]
    fn live() { assert_eq!(super::live(20), 41); }
}
"#,
        r#"fn helper(x: i32) -> i32 { x * 2 }
pub fn live(x: i32) -> i32 { helper(x) + 1 }
#[cfg(test)]
mod tests {
    #[test]
    fn live() { assert_eq!(super::live(20), 41); }
}
"#,
        &[],
    );
}

#[test]
fn nothing_outside_the_library() {
    let root = scratch("functions-library");
    let manifest = "[package]\nname = \"p\"\nedition = \"2024\"\n";
    fs::write(root.join("Cargo.toml"), manifest).unwrap();
    for dir in ["src", "tests", "examples"] {
        fs::create_dir(root.join(dir)).unwrap();
    }
    let files = [
        ("src/lib.rs", "pub fn live() {}\nfn dead() {}\n"),
        ("src/main.rs", "fn main() {}\nfn dead_in_bin() {}\n"),
        ("tests/t.rs", "fn dead_in_test() {}\n"),
        ("examples/e.rs", "fn main() {}\nfn dead_in_example() {}\n"),
    ];
    for (path, code) in files {
        fs::write(root.join(path), code).unwrap();
    }

    minify(&root, &["--kinds", "function"]);

    for (path, code) in files {
        let expected = if path == "src/lib.rs" {
            "pub fn live() {}\n"
        } else {
            code
        };
        assert_eq!(
            fs::read_to_string(root.join(path)).unwrap(),
            expected,
            "{path}"
        );
    }
    cargo(&root, &["build", "--all-targets", "--quiet"]);
}
