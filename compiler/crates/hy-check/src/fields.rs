//! Struct-literal field names against the DECLARED struct (issue #152, slice 2).
//!
//! `hyb check` resolved names, but never related an expression to the declared
//! shape it must match. A struct literal is the cheapest case to close: the
//! literal states its own type, so the field names can be compared with no
//! inference at all.
//!
//! ```hy
//! struct titik {
//!     x int
//!     y int
//! }
//!
//! fn main() {
//!     p := titik{x: 1, z: 2}
//!     fmt.println("{p.x}")
//! }
//! ```
//!
//! ```text
//! $ hyb check field_hantu.hy   -> field_hantu.hy: OK (0 diagnostics)
//! $ go build ./...             -> ./x.go:13:19: unknown field z in struct literal of type titik
//! ```
//!
//! The same holds for a field given twice (Go: `duplicate field x in struct
//! literal`). Both are certain and local.
//!
//! Scope is deliberately narrow, because a false reject is far worse than a
//! missed one: the literal must name its struct with a **bare, non-dotted**
//! identifier, and that struct must be declared in the same file. Qualified
//! (`pkg.thing{...}`) and type-elided (`{x: 1}`) literals are skipped, and a
//! **partial** field set is silent — Go allows naming a subset.

use hy_syntax::{FieldInit, TypeExpr};

/// Why a struct literal does not match its declared struct.
pub enum FieldError {
    /// A field name the struct does not declare.
    Unknown { name: String },
    /// The same field given more than once (Go: "duplicate field").
    Duplicate { name: String },
}

/// The struct name of a struct literal, when the literal names it with a
/// **bare** identifier (`titik{x: 1}`). Dotted type paths (`pkg.thing{...}`)
/// are foreign and type-elided literals carry no name; both return `None`.
pub fn bare_struct_lit_type(ty: &Option<TypeExpr>) -> Option<&str> {
    match ty.as_ref()? {
        TypeExpr::Named { name, .. } if !name.contains('.') => Some(name.as_str()),
        _ => None,
    }
}

/// Check a struct literal's field names against the DECLARED fields.
///
/// `declared` is the struct's field list as `(name, type)` pairs. Returns the
/// first problem in source order, or `None` when the literal is consistent.
/// An empty `declared` list means the struct is unknown here (zero-field alias
/// declarations are never registered as containers) and is never reported.
pub fn check_struct_lit_fields(
    declared: &[(String, TypeExpr)],
    fields: &[FieldInit],
) -> Option<FieldError> {
    if declared.is_empty() {
        return None;
    }
    let mut seen: Vec<&str> = Vec::new();
    for f in fields {
        if declared.iter().any(|(n, _)| n == &f.name) {
            if seen.contains(&f.name.as_str()) {
                return Some(FieldError::Duplicate {
                    name: f.name.clone(),
                });
            }
            seen.push(f.name.as_str());
        } else {
            return Some(FieldError::Unknown {
                name: f.name.clone(),
            });
        }
    }
    None
}

/// `a, b, c` — the declared field names in order, for a diagnostic note.
pub fn render_field_names(declared: &[(String, TypeExpr)]) -> String {
    declared
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hy_syntax::{Decl, Expr, Stmt};

    fn struct_fields(src: &str) -> Vec<(String, TypeExpr)> {
        let e = hy_syntax::parse_file("t.hy", src).expect("parse");
        for d in &e.decls {
            if let Decl::Struct(s) = d {
                return s.fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
            }
        }
        panic!("no struct decl")
    }

    fn lit_fields(src: &str) -> (Option<TypeExpr>, Vec<FieldInit>) {
        let e = hy_syntax::parse_file("t.hy", src).expect("parse");
        let f = e.fns().next().unwrap();
        let stmt = &f.body.as_ref().unwrap().stmts[0];
        if let Stmt::Local(l) = stmt {
            if let Expr::StructLit { ty, fields, .. } = &l.values[0] {
                return (ty.clone(), fields.clone());
            }
        }
        panic!("expected a struct literal local")
    }

    const TITIK: &str = "struct titik {\n    x int\n    y int\n}\nfn main() { p := titik{x: 1, y: 2} }";

    #[test]
    fn unknown_field_in_struct_literal_is_reported() {
        let d = struct_fields(TITIK);
        let (ty, f) = lit_fields(
            "struct titik {\n    x int\n    y int\n}\nfn main() { p := titik{x: 1, z: 2} }",
        );
        assert_eq!(bare_struct_lit_type(&ty), Some("titik"));
        match check_struct_lit_fields(&d, &f) {
            Some(FieldError::Unknown { name }) => assert_eq!(name, "z"),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn duplicate_field_in_struct_literal_is_reported() {
        let d = struct_fields(TITIK);
        let (_, f) = lit_fields(
            "struct titik {\n    x int\n    y int\n}\nfn main() { p := titik{x: 1, x: 2} }",
        );
        match check_struct_lit_fields(&d, &f) {
            Some(FieldError::Duplicate { name }) => assert_eq!(name, "x"),
            _ => panic!("expected Duplicate"),
        }
    }

    #[test]
    fn exact_fields_are_silent() {
        let d = struct_fields(TITIK);
        let (_, f) = lit_fields(TITIK);
        assert!(check_struct_lit_fields(&d, &f).is_none());
    }

    #[test]
    fn partial_field_set_is_silent_go_allows_it() {
        let d = struct_fields(TITIK);
        let (_, f) = lit_fields(
            "struct titik {\n    x int\n    y int\n}\nfn main() { p := titik{x: 1} }",
        );
        assert!(check_struct_lit_fields(&d, &f).is_none());
    }

    #[test]
    fn dotted_and_elided_literals_are_skipped() {
        let (ty_dot, _) = lit_fields("fn main() { p := pkg.titik{x: 1} }");
        assert!(bare_struct_lit_type(&ty_dot).is_none(), "dotted type is foreign");
        let (ty_none, _) = lit_fields("fn main() { p := {x: 1} }");
        assert!(bare_struct_lit_type(&ty_none).is_none(), "elided type");
    }

    #[test]
    fn unknown_struct_is_never_reported() {
        let (_, f) = lit_fields("fn main() { p := titik{x: 1, z: 2} }");
        assert!(
            check_struct_lit_fields(&[], &f).is_none(),
            "an unregistered struct stays permissive"
        );
    }
}
