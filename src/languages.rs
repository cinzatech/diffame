//! Built-in [`LanguageProfile`] implementations for all supported tree-sitter
//! grammars and a file-extension based lookup.
//!
//! All wiring lives in the [`define_languages!`] invocation at the bottom of
//! this file.  To add a new language: add one `simple_profile!` line and one
//! entry in the macro table.  Both `profile_for_ext` and
//! `supported_extensions` are generated automatically, there is no second
//! list to keep in sync.

use tree_sitter::Language;

use crate::language::LanguageProfile;

macro_rules! simple_profile {
    ($name:ident, $lang_expr:expr) => {
        pub struct $name;
        impl LanguageProfile for $name {
            fn language(&self) -> Language {
                $lang_expr.into()
            }
        }
    };
}

simple_profile!(AgdaProfile, tree_sitter_agda::LANGUAGE);
simple_profile!(BashProfile, tree_sitter_bash::LANGUAGE);
simple_profile!(CProfile, tree_sitter_c::LANGUAGE);
simple_profile!(CppProfile, tree_sitter_cpp::LANGUAGE);
simple_profile!(CSharpProfile, tree_sitter_c_sharp::LANGUAGE);
simple_profile!(CssProfile, tree_sitter_css::LANGUAGE);
simple_profile!(
    EmbeddedTemplateProfile,
    tree_sitter_embedded_template::LANGUAGE
);
simple_profile!(GoProfile, tree_sitter_go::LANGUAGE);
simple_profile!(HaskellProfile, tree_sitter_haskell::LANGUAGE);
simple_profile!(HtmlProfile, tree_sitter_html::LANGUAGE);
simple_profile!(JavaProfile, tree_sitter_java::LANGUAGE);
simple_profile!(JavaScriptProfile, tree_sitter_javascript::LANGUAGE);
simple_profile!(JsDocProfile, tree_sitter_jsdoc::LANGUAGE);
simple_profile!(JsonProfile, tree_sitter_json::LANGUAGE);
simple_profile!(JuliaProfile, tree_sitter_julia::LANGUAGE);
simple_profile!(OcamlProfile, tree_sitter_ocaml::LANGUAGE_OCAML);
simple_profile!(
    OcamlInterfaceProfile,
    tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE
);
simple_profile!(PhpProfile, tree_sitter_php::LANGUAGE_PHP);
simple_profile!(PythonProfile, tree_sitter_python::LANGUAGE);
simple_profile!(RegexProfile, tree_sitter_regex::LANGUAGE);
simple_profile!(RubyProfile, tree_sitter_ruby::LANGUAGE);
simple_profile!(RustProfile, tree_sitter_rust::LANGUAGE);
simple_profile!(ScalaProfile, tree_sitter_scala::LANGUAGE);
simple_profile!(
    TypeScriptProfile,
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT
);
simple_profile!(TsxProfile, tree_sitter_typescript::LANGUAGE_TSX);
simple_profile!(VerilogProfile, tree_sitter_verilog::LANGUAGE);
simple_profile!(YamlProfile, tree_sitter_yaml::LANGUAGE);

simple_profile!(CmakeProfile, tree_sitter_cmake::LANGUAGE);
simple_profile!(
    CommonLispProfile,
    tree_sitter_commonlisp::LANGUAGE_COMMONLISP
);
simple_profile!(DartProfile, tree_sitter_dart::LANGUAGE);
simple_profile!(DockerfileProfile, tree_sitter_containerfile::LANGUAGE);
simple_profile!(ElixirProfile, tree_sitter_elixir::LANGUAGE);
simple_profile!(ElmProfile, tree_sitter_elm::LANGUAGE);
simple_profile!(ErlangProfile, tree_sitter_erlang::LANGUAGE);
simple_profile!(FortranProfile, tree_sitter_fortran::LANGUAGE);
simple_profile!(GdScriptProfile, tree_sitter_gdscript::LANGUAGE);
simple_profile!(GlslProfile, tree_sitter_glsl::LANGUAGE_GLSL);
simple_profile!(GraphqlProfile, tree_sitter_graphql::LANGUAGE);
simple_profile!(GroovyProfile, tree_sitter_groovy::LANGUAGE);
simple_profile!(HclProfile, tree_sitter_hcl::LANGUAGE);
simple_profile!(KotlinProfile, tree_sitter_kotlin_ng::LANGUAGE);
simple_profile!(LatexProfile, tree_sitter_latex::LANGUAGE);
simple_profile!(LuaProfile, tree_sitter_lua::LANGUAGE);
simple_profile!(MakeProfile, tree_sitter_make::LANGUAGE);
simple_profile!(MarkdownProfile, tree_sitter_md::LANGUAGE);
simple_profile!(NginxProfile, tree_sitter_nginx::LANGUAGE);
simple_profile!(NixProfile, tree_sitter_nix::LANGUAGE);
simple_profile!(ObjectiveCProfile, tree_sitter_objc::LANGUAGE);
simple_profile!(PascalProfile, tree_sitter_pascal::LANGUAGE);
simple_profile!(PerlProfile, tree_sitter_perl::LANGUAGE);
simple_profile!(PowerShellProfile, tree_sitter_powershell::LANGUAGE);
simple_profile!(PrologProfile, tree_sitter_prolog::LANGUAGE);
simple_profile!(ProtoProfile, tree_sitter_proto::LANGUAGE);
simple_profile!(RProfile, tree_sitter_r::LANGUAGE);
simple_profile!(RacketProfile, tree_sitter_racket::LANGUAGE);
simple_profile!(SchemeProfile, tree_sitter_scheme::LANGUAGE);
simple_profile!(SolidityProfile, tree_sitter_solidity::LANGUAGE);
simple_profile!(SqlProfile, tree_sitter_sql::LANGUAGE);
simple_profile!(SwiftProfile, tree_sitter_swift::LANGUAGE);
simple_profile!(TomlProfile, tree_sitter_toml_ng::LANGUAGE);
simple_profile!(XmlProfile, tree_sitter_xml::LANGUAGE_XML);
simple_profile!(DtdProfile, tree_sitter_xml::LANGUAGE_DTD);
simple_profile!(ZigProfile, tree_sitter_zig::LANGUAGE);

// `define_languages!` generates both `profile_for_ext` and
// `supported_extensions` from this one table.  Adding a language is a
// single-line change.
//
// NOTE on ambiguous extensions:
//   - `.h` is mapped to C, but could be C++ or Objective-C.  Pass `-l cpp`
//     or `-l m` to override.
//   - `.conf` is intentionally omitted: it is used by Nginx, Apache, systemd,
//     and many others.  Use `-l nginx` explicitly.
//   - `.m` is intentionally omitted: it is MATLAB in scientific codebases and
//     Objective-C elsewhere.  Use `-l objc` explicitly.

macro_rules! define_languages {
    ( $( $static_name:ident : $profile_ty:ident, $display:literal => [ $( $ext:literal ),+ ] ; )* ) => {
        /// Return a `&dyn LanguageProfile` for a file extension (without the
        /// leading dot), or `None` if the extension is not recognised.
        pub fn profile_for_ext(ext: &str) -> Option<&'static dyn LanguageProfile> {
            $( static $static_name: $profile_ty = $profile_ty; )*
            match ext {
                $( $( $ext )|+ => Some(&$static_name), )*
                _ => None,
            }
        }

        /// List all supported file extensions (auto-generated from the
        /// registry; order matches declaration order).
        pub fn supported_extensions() -> &'static [&'static str] {
            &[ $( $( $ext, )+ )* ]
        }

        /// Return a human-readable language name for a file extension, or
        /// `None` if the extension is not recognised.
        pub fn language_name_for_ext(ext: &str) -> Option<&'static str> {
            match ext {
                $( $( $ext )|+ => Some($display), )*
                _ => None,
            }
        }
    };
}

define_languages! {
    AGDA:       AgdaProfile              , "Agda" => ["agda"];
    BASH:       BashProfile              , "Bash" => ["sh", "bash", "zsh"];
    C:          CProfile                 , "C" => ["c", "h"];
    CMAKE:      CmakeProfile             , "CMake" => ["cmake"];
    COMMONLISP: CommonLispProfile        , "Common Lisp" => ["cl", "lisp", "lsp", "asd"];
    CPP:        CppProfile               , "C++" => ["cc", "cpp", "cxx", "hpp", "hxx", "hh"];
    CSHARP:     CSharpProfile            , "C#" => ["cs"];
    CSS:        CssProfile               , "CSS" => ["css"];
    DART:       DartProfile              , "Dart" => ["dart"];
    DOCKERFILE: DockerfileProfile        , "Dockerfile" => ["dockerfile"];
    DTD:        DtdProfile               , "DTD" => ["dtd"];
    ELIXIR:     ElixirProfile            , "Elixir" => ["ex", "exs"];
    ELM:        ElmProfile               , "Elm" => ["elm"];
    ERB:        EmbeddedTemplateProfile  , "ERB" => ["erb", "ejs"];
    ERLANG:     ErlangProfile            , "Erlang" => ["erl", "hrl"];
    FORTRAN:    FortranProfile           , "Fortran" => ["f", "f90", "f95", "f03", "f08", "for", "fpp"];
    GDSCRIPT:   GdScriptProfile          , "GDScript" => ["gd"];
    GLSL:       GlslProfile              , "GLSL" => ["glsl", "vert", "frag", "geom", "comp"];
    GO:         GoProfile                , "Go" => ["go"];
    GRAPHQL:    GraphqlProfile           , "GraphQL" => ["graphql", "gql"];
    GROOVY:     GroovyProfile            , "Groovy" => ["groovy", "gradle"];
    HASKELL:    HaskellProfile           , "Haskell" => ["hs", "lhs"];
    HCL:        HclProfile               , "HCL" => ["hcl", "tf", "tfvars"];
    HTML:       HtmlProfile              , "HTML" => ["html", "htm"];
    JAVA:       JavaProfile              , "Java" => ["java"];
    JS:         JavaScriptProfile        , "JavaScript" => ["js", "mjs", "cjs", "jsx"];
    JSDOC:      JsDocProfile             , "JSDoc" => ["jsdoc"];
    JSON:       JsonProfile              , "JSON" => ["json"];
    JULIA:      JuliaProfile             , "Julia" => ["jl"];
    KOTLIN:     KotlinProfile            , "Kotlin" => ["kt", "kts"];
    LATEX:      LatexProfile             , "LaTeX" => ["tex", "latex", "sty", "cls"];
    LUA:        LuaProfile               , "Lua" => ["lua"];
    MAKE:       MakeProfile              , "Make" => ["mk"];
    MARKDOWN:   MarkdownProfile          , "Markdown" => ["md", "markdown"];
    NGINX:      NginxProfile             , "Nginx" => ["nginx"];
    NIX:        NixProfile               , "Nix" => ["nix"];
    OBJC:       ObjectiveCProfile        , "Objective-C" => ["objc"];
    OCAML:      OcamlProfile             , "OCaml" => ["ml"];
    OCAML_IF:   OcamlInterfaceProfile    , "OCaml Interface" => ["mli"];
    PASCAL:     PascalProfile            , "Pascal" => ["pas", "pp", "lpr", "dpr"];
    PERL:       PerlProfile              , "Perl" => ["pl", "pm"];
    PHP:        PhpProfile               , "PHP" => ["php"];
    POWERSHELL: PowerShellProfile        , "PowerShell" => ["ps1", "psm1", "psd1"];
    PROLOG:     PrologProfile            , "Prolog" => ["pro", "P", "prolog"];
    PROTO:      ProtoProfile             , "Protocol Buffers" => ["proto"];
    PYTHON:     PythonProfile            , "Python" => ["py", "pyi"];
    R:          RProfile                 , "R" => ["r", "R"];
    RACKET:     RacketProfile            , "Racket" => ["rkt"];
    REGEX:      RegexProfile             , "Regex" => ["regex"];
    RUBY:       RubyProfile              , "Ruby" => ["rb"];
    RUST:       RustProfile              , "Rust" => ["rs"];
    SCALA:      ScalaProfile             , "Scala" => ["scala", "sc"];
    SCHEME:     SchemeProfile            , "Scheme" => ["scm", "ss"];
    SOLIDITY:   SolidityProfile          , "Solidity" => ["sol"];
    SQL:        SqlProfile               , "SQL" => ["sql"];
    SWIFT:      SwiftProfile             , "Swift" => ["swift"];
    TOML:       TomlProfile              , "TOML" => ["toml"];
    TS:         TypeScriptProfile        , "TypeScript" => ["ts", "mts", "cts"];
    TSX:        TsxProfile               , "TSX" => ["tsx"];
    VERILOG:    VerilogProfile           , "Verilog" => ["v", "sv", "svh"];
    XML:        XmlProfile               , "XML" => ["xml", "xsl", "xslt", "xsd", "svg", "plist"];
    YAML:       YamlProfile              , "YAML" => ["yaml", "yml"];
    ZIG:        ZigProfile               , "Zig" => ["zig"];
}

/// Look up a profile by **filename** (e.g. `Dockerfile`, `Makefile`).
/// Returns `None` for filenames that don't have a special mapping.
#[must_use]
pub fn profile_for_filename(name: &str) -> Option<&'static dyn LanguageProfile> {
    // Case-insensitive match for well-known extensionless filenames.
    match name.to_ascii_lowercase().as_str() {
        "dockerfile" | "containerfile" => profile_for_ext("dockerfile"),
        "makefile" | "gnumakefile" => profile_for_ext("mk"),
        _ => None,
    }
}
