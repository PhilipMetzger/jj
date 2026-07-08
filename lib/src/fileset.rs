// Copyright 2024 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Functional language for selecting a set of paths.

use std::collections::HashMap;
use std::sync::LazyLock;

use itertools::Itertools as _;
pub use jj_core::fileset::FilePattern;
pub use jj_core::fileset::FilePatternParseError;
pub use jj_core::fileset::FilesMatcher;
pub use jj_core::fileset::FilesetExpression;
pub use jj_core::fileset::parse_file_glob;

use crate::dsl_util::collect_similar;
use crate::fileset_parser;
use crate::fileset_parser::BinaryOp;
use crate::fileset_parser::ExpressionKind;
use crate::fileset_parser::ExpressionNode;
pub use crate::fileset_parser::FilesetAliasesMap;
pub use crate::fileset_parser::FilesetDiagnostics;
pub use crate::fileset_parser::FilesetParseError;
pub use crate::fileset_parser::FilesetParseErrorKind;
pub use crate::fileset_parser::FilesetParseResult;
use crate::fileset_parser::FunctionCallNode;
use crate::fileset_parser::UnaryOp;
use crate::repo_path::RepoPathUiConverter;

type FilesetFunction = fn(
    &mut FilesetDiagnostics,
    &RepoPathUiConverter,
    &FunctionCallNode,
) -> FilesetParseResult<FilesetExpression>;

static BUILTIN_FUNCTION_MAP: LazyLock<HashMap<&str, FilesetFunction>> = LazyLock::new(|| {
    // Not using maplit::hashmap!{} or custom declarative macro here because
    // code completion inside macro is quite restricted.
    let mut map: HashMap<&str, FilesetFunction> = HashMap::new();
    map.insert("none", |_diagnostics, _path_converter, function| {
        function.expect_no_arguments()?;
        Ok(FilesetExpression::none())
    });
    map.insert("all", |_diagnostics, _path_converter, function| {
        function.expect_no_arguments()?;
        Ok(FilesetExpression::all())
    });
    map
});

fn resolve_function(
    diagnostics: &mut FilesetDiagnostics,
    path_converter: &RepoPathUiConverter,
    function: &FunctionCallNode,
) -> FilesetParseResult<FilesetExpression> {
    if let Some(func) = BUILTIN_FUNCTION_MAP.get(function.name) {
        func(diagnostics, path_converter, function)
    } else {
        Err(FilesetParseError::new(
            FilesetParseErrorKind::NoSuchFunction {
                name: function.name.to_owned(),
                candidates: collect_similar(function.name, BUILTIN_FUNCTION_MAP.keys()),
            },
            function.name_span,
        ))
    }
}

fn resolve_expression(
    diagnostics: &mut FilesetDiagnostics,
    path_converter: &RepoPathUiConverter,
    node: &ExpressionNode,
) -> FilesetParseResult<FilesetExpression> {
    fileset_parser::catch_aliases(diagnostics, node, |diagnostics, node| {
        let wrap_pattern_error =
            |err| FilesetParseError::expression("Invalid file pattern", node.span).with_source(err);
        match &node.kind {
            ExpressionKind::Identifier(name) => {
                let pattern = FilePattern::cwd_prefix_glob(path_converter, name)
                    .map_err(wrap_pattern_error)?;
                Ok(FilesetExpression::pattern(pattern))
            }
            ExpressionKind::String(name) => {
                let pattern = FilePattern::cwd_prefix_glob(path_converter, name)
                    .map_err(wrap_pattern_error)?;
                Ok(FilesetExpression::pattern(pattern))
            }
            ExpressionKind::Pattern(pattern) => {
                let value = fileset_parser::expect_string_literal("string", &pattern.value)?;
                let pattern = FilePattern::from_str_kind(path_converter, value, pattern.name)
                    .map_err(wrap_pattern_error)?;
                Ok(FilesetExpression::pattern(pattern))
            }
            ExpressionKind::Unary(op, arg_node) => {
                let arg = resolve_expression(diagnostics, path_converter, arg_node)?;
                match op {
                    UnaryOp::Negate => Ok(FilesetExpression::all().difference(arg)),
                }
            }
            ExpressionKind::Binary(op, lhs_node, rhs_node) => {
                let lhs = resolve_expression(diagnostics, path_converter, lhs_node)?;
                let rhs = resolve_expression(diagnostics, path_converter, rhs_node)?;
                match op {
                    BinaryOp::Intersection => Ok(lhs.intersection(rhs)),
                    BinaryOp::Difference => Ok(lhs.difference(rhs)),
                }
            }
            ExpressionKind::UnionAll(nodes) => {
                let expressions = nodes
                    .iter()
                    .map(|node| resolve_expression(diagnostics, path_converter, node))
                    .try_collect()?;
                Ok(FilesetExpression::union_all(expressions))
            }
            ExpressionKind::FunctionCall(function) => {
                resolve_function(diagnostics, path_converter, function)
            }
            ExpressionKind::AliasExpanded(..) => unreachable!(),
        }
    })
}

/// Information needed to parse fileset expression.
#[derive(Clone, Debug)]
pub struct FilesetParseContext<'a> {
    /// Aliases to be expanded.
    pub aliases_map: &'a FilesetAliasesMap,
    /// Context to resolve cwd-relative paths.
    pub path_converter: &'a RepoPathUiConverter,
}

/// Parses text into `FilesetExpression` without bare string fallback.
pub fn parse(
    diagnostics: &mut FilesetDiagnostics,
    text: &str,
    context: &FilesetParseContext,
) -> FilesetParseResult<FilesetExpression> {
    let node = fileset_parser::parse_program(text)?;
    let node = fileset_parser::expand_aliases(node, context.aliases_map)?;
    // TODO: add basic tree substitution pass to eliminate redundant expressions
    resolve_expression(diagnostics, context.path_converter, &node)
}

/// Parses text into `FilesetExpression` with bare string fallback.
///
/// If the text can't be parsed as a fileset expression, and if it doesn't
/// contain any operator-like characters, it will be parsed as a file path.
pub fn parse_maybe_bare(
    diagnostics: &mut FilesetDiagnostics,
    text: &str,
    context: &FilesetParseContext,
) -> FilesetParseResult<FilesetExpression> {
    let node = fileset_parser::parse_program_or_bare_string(text)?;
    let node = fileset_parser::expand_aliases(node, context.aliases_map)?;
    // TODO: add basic tree substitution pass to eliminate redundant expressions
    resolve_expression(diagnostics, context.path_converter, &node)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::tests::TestResult;

    fn repo_path_buf(value: impl Into<String>) -> RepoPathBuf {
        RepoPathBuf::from_internal_string(value).unwrap()
    }

    fn insta_settings() -> insta::Settings {
        let mut settings = insta::Settings::clone_current();
        // Elide parsed glob options and tokens, which aren't interesting.
        settings.add_filter(
            r"(?m)^(\s{12}opts):\s*GlobOptions\s*\{\n(\s{16}.*\n)*\s{12}\},",
            "$1: _,",
        );
        settings.add_filter(
            r"(?m)^(\s{12}tokens):\s*Tokens\(\n(\s{16}.*\n)*\s{12}\),",
            "$1: _,",
        );
        // Collapse short "Thing(_,)" repeatedly to save vertical space and make
        // the output more readable.
        for _ in 0..4 {
            settings.add_filter(
                r"(?x)
                \b([A-Z]\w*)\(\n
                    \s*(.{1,60}),\n
                \s*\)",
                "$1($2)",
            );
        }
        settings
    }

    #[test]
    fn test_parse_file_pattern() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                cwd: PathBuf::from("/ws/cur"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        // cwd-relative patterns
        insta::assert_debug_snapshot!(
            parse(".")?,
            @r#"Pattern(PrefixPath("cur"))"#);
        insta::assert_debug_snapshot!(
            parse("..")?,
            @r#"Pattern(PrefixPath(""))"#);
        assert!(parse("../..").is_err());
        insta::assert_debug_snapshot!(
            parse("foo")?,
            @r#"Pattern(PrefixPath("cur/foo"))"#);
        insta::assert_debug_snapshot!(
            parse("*.*")?,
            @r#"
        Pattern(
            PrefixGlob {
                dir: "cur",
                pattern: Glob {
                    glob: "*.*",
                    re: "(?-u)^[^/]*\\.[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse("cwd:.")?,
            @r#"Pattern(PrefixPath("cur"))"#);
        insta::assert_debug_snapshot!(
            parse("cwd-file:foo")?,
            @r#"Pattern(FilePath("cur/foo"))"#);
        insta::assert_debug_snapshot!(
            parse("file:../foo/bar")?,
            @r#"Pattern(FilePath("foo/bar"))"#);

        // workspace-relative patterns
        insta::assert_debug_snapshot!(
            parse("root:.")?,
            @r#"Pattern(PrefixPath(""))"#);
        assert!(parse("root:..").is_err());
        insta::assert_debug_snapshot!(
            parse("root:foo/bar")?,
            @r#"Pattern(PrefixPath("foo/bar"))"#);
        insta::assert_debug_snapshot!(
            parse("root-file:bar")?,
            @r#"Pattern(FilePath("bar"))"#);

        insta::assert_debug_snapshot!(
            parse("file:(foo|bar)").unwrap_err().kind(),
            @r#"Expression("Expected string")"#);
        Ok(())
    }

    #[test]
    fn test_parse_glob_pattern() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                // meta character in cwd path shouldn't be expanded
                cwd: PathBuf::from("/ws/cur*"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        // cwd-relative, without meta characters
        insta::assert_debug_snapshot!(
            parse(r#"cwd-glob:"foo""#)?,
            @r#"Pattern(FilePath("cur*/foo"))"#);
        // Strictly speaking, glob:"" shouldn't match a file named <cwd>, but
        // file pattern doesn't distinguish "foo/" from "foo".
        insta::assert_debug_snapshot!(
            parse(r#"glob:"""#)?,
            @r#"Pattern(FilePath("cur*"))"#);
        insta::assert_debug_snapshot!(
            parse(r#"glob:".""#)?,
            @r#"Pattern(FilePath("cur*"))"#);
        insta::assert_debug_snapshot!(
            parse(r#"glob:"..""#)?,
            @r#"Pattern(FilePath(""))"#);

        // cwd-relative, with meta characters
        insta::assert_debug_snapshot!(
            parse(r#"glob:"*""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur*",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse(r#"glob:"./*""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur*",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse(r#"glob:"../*""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        // glob:"**" is equivalent to root-glob:"<cwd>/**", not root-glob:"**"
        insta::assert_debug_snapshot!(
            parse(r#"glob:"**""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur*",
                pattern: Glob {
                    glob: "**",
                    re: "(?-u)^.*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse(r#"glob:"../foo/b?r/baz""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "foo",
                pattern: Glob {
                    glob: "b?r/baz",
                    re: "(?-u)^b[^/]r/baz$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        assert!(parse(r#"glob:"../../*""#).is_err());
        assert!(parse(r#"glob-i:"../../*""#).is_err());
        assert!(parse(r#"glob:"/*""#).is_err());
        assert!(parse(r#"glob-i:"/*""#).is_err());
        // no support for relative path component after glob meta character
        assert!(parse(r#"glob:"*/..""#).is_err());
        assert!(parse(r#"glob-i:"*/..""#).is_err());

        if cfg!(windows) {
            // cwd-relative, with Windows path separators
            insta::assert_debug_snapshot!(
                parse(r#"glob:"..\\foo\\*\\bar""#)?, @r#"
            Pattern(
                FileGlob {
                    dir: "foo",
                    pattern: Glob {
                        glob: "*/bar",
                        re: "(?-u)^[^/]*/bar$",
                        opts: _,
                        tokens: _,
                    },
                },
            )
            "#);
        } else {
            // backslash is an escape character on Unix
            insta::assert_debug_snapshot!(
                parse(r#"glob:"..\\foo\\*\\bar""#)?, @r#"
            Pattern(
                FileGlob {
                    dir: "cur*",
                    pattern: Glob {
                        glob: "..\\foo\\*\\bar",
                        re: "(?-u)^\\.\\.foo\\*bar$",
                        opts: _,
                        tokens: _,
                    },
                },
            )
            "#);
        }

        // workspace-relative, without meta characters
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:"foo""#)?,
            @r#"Pattern(FilePath("foo"))"#);
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:"""#)?,
            @r#"Pattern(FilePath(""))"#);
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:".""#)?,
            @r#"Pattern(FilePath(""))"#);

        // workspace-relative, with meta characters
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:"*""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:"foo/bar/b[az]""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "foo/bar",
                pattern: Glob {
                    glob: "b[az]",
                    re: "(?-u)^b[az]$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        insta::assert_debug_snapshot!(
            parse(r#"root-glob:"foo/bar/b{ar,az}""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "foo/bar",
                pattern: Glob {
                    glob: "b{ar,az}",
                    re: "(?-u)^b(?:ar|az)$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        assert!(parse(r#"root-glob:"../*""#).is_err());
        assert!(parse(r#"root-glob-i:"../*""#).is_err());
        assert!(parse(r#"root-glob:"/*""#).is_err());
        assert!(parse(r#"root-glob-i:"/*""#).is_err());

        // workspace-relative, backslash escape without meta characters
        if cfg!(not(windows)) {
            insta::assert_debug_snapshot!(
                parse(r#"root-glob:'foo/bar\baz'"#)?, @r#"
            Pattern(
                FileGlob {
                    dir: "foo",
                    pattern: Glob {
                        glob: "bar\\baz",
                        re: "(?-u)^barbaz$",
                        opts: _,
                        tokens: _,
                    },
                },
            )
            "#);
        }
        Ok(())
    }

    #[test]
    fn test_parse_glob_pattern_case_insensitive() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                cwd: PathBuf::from("/ws/cur"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        // cwd-relative case-insensitive glob
        insta::assert_debug_snapshot!(
            parse(r#"glob-i:"*.TXT""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur",
                pattern: Glob {
                    glob: "*.TXT",
                    re: "(?-u)(?i)^[^/]*\\.TXT$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // cwd-relative case-insensitive glob with more specific pattern
        insta::assert_debug_snapshot!(
            parse(r#"cwd-glob-i:"[Ff]oo""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur",
                pattern: Glob {
                    glob: "[Ff]oo",
                    re: "(?-u)(?i)^[Ff]oo$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // workspace-relative case-insensitive glob
        insta::assert_debug_snapshot!(
            parse(r#"root-glob-i:"*.Rs""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "",
                pattern: Glob {
                    glob: "*.Rs",
                    re: "(?-u)(?i)^[^/]*\\.Rs$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // case-insensitive pattern with directory component (should not split the path)
        insta::assert_debug_snapshot!(
            parse(r#"glob-i:"SubDir/*.rs""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur",
                pattern: Glob {
                    glob: "SubDir/*.rs",
                    re: "(?-u)(?i)^SubDir/[^/]*\\.rs$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // case-sensitive pattern with directory component (should split the path)
        insta::assert_debug_snapshot!(
            parse(r#"glob:"SubDir/*.rs""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur/SubDir",
                pattern: Glob {
                    glob: "*.rs",
                    re: "(?-u)^[^/]*\\.rs$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // case-insensitive pattern with leading dots (should split dots but not dirs)
        insta::assert_debug_snapshot!(
            parse(r#"glob-i:"../SomeDir/*.rs""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "",
                pattern: Glob {
                    glob: "SomeDir/*.rs",
                    re: "(?-u)(?i)^SomeDir/[^/]*\\.rs$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // case-insensitive pattern with single leading dot
        insta::assert_debug_snapshot!(
            parse(r#"glob-i:"./SomeFile*.txt""#)?, @r#"
        Pattern(
            FileGlob {
                dir: "cur",
                pattern: Glob {
                    glob: "SomeFile*.txt",
                    re: "(?-u)(?i)^SomeFile[^/]*\\.txt$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        Ok(())
    }

    #[test]
    fn test_parse_prefix_glob_pattern() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                // meta character in cwd path shouldn't be expanded
                cwd: PathBuf::from("/ws/cur*"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        // cwd-relative, without meta/case-insensitive characters
        insta::assert_debug_snapshot!(
            parse("cwd-prefix-glob:'foo'")?,
            @r#"Pattern(PrefixPath("cur*/foo"))"#);
        insta::assert_debug_snapshot!(
            parse("prefix-glob:'.'")?,
            @r#"Pattern(PrefixPath("cur*"))"#);
        insta::assert_debug_snapshot!(
            parse("cwd-prefix-glob-i:'..'")?,
            @r#"Pattern(PrefixPath(""))"#);
        insta::assert_debug_snapshot!(
            parse("prefix-glob-i:'../_'")?,
            @r#"Pattern(PrefixPath("_"))"#);

        // cwd-relative, with meta characters
        insta::assert_debug_snapshot!(
            parse("cwd-prefix-glob:'*'")?, @r#"
        Pattern(
            PrefixGlob {
                dir: "cur*",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // cwd-relative, with case-insensitive characters
        insta::assert_debug_snapshot!(
            parse("cwd-prefix-glob-i:'../foo'")?, @r#"
        Pattern(
            PrefixGlob {
                dir: "",
                pattern: Glob {
                    glob: "foo",
                    re: "(?-u)(?i)^foo$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // workspace-relative, without meta/case-insensitive characters
        insta::assert_debug_snapshot!(
            parse("root-prefix-glob:'foo'")?,
            @r#"Pattern(PrefixPath("foo"))"#);
        insta::assert_debug_snapshot!(
            parse("root-prefix-glob-i:'.'")?,
            @r#"Pattern(PrefixPath(""))"#);

        // workspace-relative, with meta characters
        insta::assert_debug_snapshot!(
            parse("root-prefix-glob:'*'")?, @r#"
        Pattern(
            PrefixGlob {
                dir: "",
                pattern: Glob {
                    glob: "*",
                    re: "(?-u)^[^/]*$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);

        // workspace-relative, with case-insensitive characters
        insta::assert_debug_snapshot!(
            parse("root-prefix-glob-i:'_/foo'")?, @r#"
        Pattern(
            PrefixGlob {
                dir: "_",
                pattern: Glob {
                    glob: "foo",
                    re: "(?-u)(?i)^foo$",
                    opts: _,
                    tokens: _,
                },
            },
        )
        "#);
        Ok(())
    }

    #[test]
    fn test_parse_function() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                cwd: PathBuf::from("/ws/cur"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        insta::assert_debug_snapshot!(parse("all()")?, @"All");
        insta::assert_debug_snapshot!(parse("none()")?, @"None");
        insta::assert_debug_snapshot!(parse("all(x)").unwrap_err().kind(), @r#"
        InvalidArguments {
            name: "all",
            message: "Expected 0 arguments",
        }
        "#);
        insta::assert_debug_snapshot!(parse("ale()").unwrap_err().kind(), @r#"
        NoSuchFunction {
            name: "ale",
            candidates: [
                "all",
            ],
        }
        "#);
        Ok(())
    }

    #[test]
    fn test_parse_compound_expression() -> TestResult {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let context = FilesetParseContext {
            aliases_map: &FilesetAliasesMap::new(),
            path_converter: &RepoPathUiConverter::Fs {
                cwd: PathBuf::from("/ws/cur"),
                base: PathBuf::from("/ws"),
            },
        };
        let parse = |text| parse_maybe_bare(&mut FilesetDiagnostics::new(), text, &context);

        insta::assert_debug_snapshot!(parse("~x")?, @r#"
        Difference(
            All,
            Pattern(PrefixPath("cur/x")),
        )
        "#);
        insta::assert_debug_snapshot!(parse("x|y|root:z")?, @r#"
        UnionAll(
            [
                Pattern(PrefixPath("cur/x")),
                Pattern(PrefixPath("cur/y")),
                Pattern(PrefixPath("z")),
            ],
        )
        "#);
        insta::assert_debug_snapshot!(parse("x|y&z")?, @r#"
        UnionAll(
            [
                Pattern(PrefixPath("cur/x")),
                Intersection(
                    Pattern(PrefixPath("cur/y")),
                    Pattern(PrefixPath("cur/z")),
                ),
            ],
        )
        "#);
        Ok(())
    }

    #[test]
    fn test_explicit_paths() {
        let collect = |expr: &FilesetExpression| -> Vec<RepoPathBuf> {
            expr.explicit_paths().map(|path| path.to_owned()).collect()
        };
        let file_expr = |path: &str| FilesetExpression::file_path(repo_path_buf(path));
        assert!(collect(&FilesetExpression::none()).is_empty());
        assert_eq!(collect(&file_expr("a")), ["a"].map(repo_path_buf));
        assert_eq!(
            collect(&FilesetExpression::union_all(vec![
                file_expr("a"),
                file_expr("b"),
                file_expr("c"),
            ])),
            ["a", "b", "c"].map(repo_path_buf)
        );
        assert_eq!(
            collect(&FilesetExpression::intersection(
                FilesetExpression::union_all(vec![
                    file_expr("a"),
                    FilesetExpression::none(),
                    file_expr("b"),
                    file_expr("c"),
                ]),
                FilesetExpression::difference(
                    file_expr("d"),
                    FilesetExpression::union_all(vec![file_expr("e"), file_expr("f")])
                )
            )),
            ["a", "b", "c", "d", "e", "f"].map(repo_path_buf)
        );
    }

    #[test]
    fn test_build_matcher_simple() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();

        insta::assert_debug_snapshot!(FilesetExpression::none().to_matcher(), @"NothingMatcher");
        insta::assert_debug_snapshot!(FilesetExpression::all().to_matcher(), @"EverythingMatcher");
        insta::assert_debug_snapshot!(
            FilesetExpression::file_path(repo_path_buf("foo")).to_matcher(),
            @r#"
        FilesMatcher {
            tree: Dir {
                "foo": File {},
            },
        }
        "#);
        insta::assert_debug_snapshot!(
            FilesetExpression::prefix_path(repo_path_buf("foo")).to_matcher(),
            @r#"
        PrefixMatcher {
            tree: Dir {
                "foo": Prefix {},
            },
        }
        "#);
    }

    #[test]
    fn test_build_matcher_glob_pattern() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();
        let file_glob_expr = |dir: &str, pattern: &str| {
            FilesetExpression::pattern(FilePattern::FileGlob {
                dir: repo_path_buf(dir),
                pattern: Box::new(parse_file_glob(pattern, false).unwrap()),
            })
        };
        let prefix_glob_expr = |dir: &str, pattern: &str| {
            FilesetExpression::pattern(FilePattern::PrefixGlob {
                dir: repo_path_buf(dir),
                pattern: Box::new(parse_file_glob(pattern, false).unwrap()),
            })
        };

        insta::assert_debug_snapshot!(file_glob_expr("", "*").to_matcher(), @r#"
        GlobsMatcher {
            tree: Some(RegexSet(["(?-u)^[^/]*$"])) {},
            matches_prefix_paths: false,
        }
        "#);

        let expr = FilesetExpression::union_all(vec![
            file_glob_expr("foo", "*"),
            file_glob_expr("foo/bar", "*"),
            file_glob_expr("foo", "?"),
            prefix_glob_expr("foo", "ba[rz]"),
            prefix_glob_expr("foo", "qu*x"),
        ]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @r#"
        UnionMatcher {
            input1: GlobsMatcher {
                tree: None {
                    "foo": Some(RegexSet(["(?-u)^[^/]*$", "(?-u)^[^/]$"])) {
                        "bar": Some(RegexSet(["(?-u)^[^/]*$"])) {},
                    },
                },
                matches_prefix_paths: false,
            },
            input2: GlobsMatcher {
                tree: None {
                    "foo": Some(RegexSet(["(?-u)^ba[rz](?:/|$)", "(?-u)^qu[^/]*x(?:/|$)"])) {},
                },
                matches_prefix_paths: true,
            },
        }
        "#);
    }

    #[test]
    fn test_build_matcher_union_patterns_of_same_kind() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();

        let expr = FilesetExpression::union_all(vec![
            FilesetExpression::file_path(repo_path_buf("foo")),
            FilesetExpression::file_path(repo_path_buf("foo/bar")),
        ]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @r#"
        FilesMatcher {
            tree: Dir {
                "foo": File {
                    "bar": File {},
                },
            },
        }
        "#);

        let expr = FilesetExpression::union_all(vec![
            FilesetExpression::prefix_path(repo_path_buf("bar")),
            FilesetExpression::prefix_path(repo_path_buf("bar/baz")),
        ]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @r#"
        PrefixMatcher {
            tree: Dir {
                "bar": Prefix {
                    "baz": Prefix {},
                },
            },
        }
        "#);
    }

    #[test]
    fn test_build_matcher_union_patterns_of_different_kind() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();

        let expr = FilesetExpression::union_all(vec![
            FilesetExpression::file_path(repo_path_buf("foo")),
            FilesetExpression::prefix_path(repo_path_buf("bar")),
        ]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @r#"
        UnionMatcher {
            input1: FilesMatcher {
                tree: Dir {
                    "foo": File {},
                },
            },
            input2: PrefixMatcher {
                tree: Dir {
                    "bar": Prefix {},
                },
            },
        }
        "#);
    }

    #[test]
    fn test_build_matcher_unnormalized_union() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();

        let expr = FilesetExpression::UnionAll(vec![]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @"NothingMatcher");

        let expr =
            FilesetExpression::UnionAll(vec![FilesetExpression::None, FilesetExpression::All]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @"
        UnionMatcher {
            input1: NothingMatcher,
            input2: EverythingMatcher,
        }
        ");
    }

    #[test]
    fn test_build_matcher_combined() {
        let settings = insta_settings();
        let _guard = settings.bind_to_scope();

        let expr = FilesetExpression::union_all(vec![
            FilesetExpression::intersection(FilesetExpression::all(), FilesetExpression::none()),
            FilesetExpression::difference(FilesetExpression::none(), FilesetExpression::all()),
            FilesetExpression::file_path(repo_path_buf("foo")),
            FilesetExpression::prefix_path(repo_path_buf("bar")),
        ]);
        insta::assert_debug_snapshot!(expr.to_matcher(), @r#"
        UnionMatcher {
            input1: UnionMatcher {
                input1: IntersectionMatcher {
                    input1: EverythingMatcher,
                    input2: NothingMatcher,
                },
                input2: DifferenceMatcher {
                    wanted: NothingMatcher,
                    unwanted: EverythingMatcher,
                },
            },
            input2: UnionMatcher {
                input1: FilesMatcher {
                    tree: Dir {
                        "foo": File {},
                    },
                },
                input2: PrefixMatcher {
                    tree: Dir {
                        "bar": Prefix {},
                    },
                },
            },
        }
        "#);
    }
}
