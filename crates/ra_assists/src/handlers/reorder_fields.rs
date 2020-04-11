use std::collections::HashMap;

use itertools::Itertools;

use hir::{Adt, ModuleDef, PathResolution, Semantics, Struct};
use ra_ide_db::RootDatabase;
use ra_syntax::{
    algo, ast,
    ast::{Name, Path, RecordLit, RecordPat},
    AstNode, SyntaxKind, SyntaxNode,
};

use crate::{
    assist_ctx::{Assist, AssistCtx},
    AssistId,
};
use ra_syntax::ast::{Expr, NameRef};


// Assist: reorder_fields
//
// Reorder the fields of record literals and record patterns in the same order as in
// the definition.
//
// ```
// struct Foo {foo: i32, bar: i32};
// const test: Foo = <|>Foo {bar: 0, foo: 1}
// ```
// ->
// ```
// struct Foo {foo: i32, bar: i32};
// const test: Foo = <|>Foo {foo: 1, bar: 0}
// ```
//
pub(crate) fn reorder_fields(ctx: AssistCtx) -> Option<Assist> {
    reorder::<RecordLit>(ctx.clone()).or_else(|| reorder::<RecordPat>(ctx))
}

fn reorder<R: AstNode>(ctx: AssistCtx) -> Option<Assist> {
    let record = ctx.find_node_at_offset::<R>()?;
    let path = record.syntax().children().find_map(Path::cast)?;

    let ranks = compute_fields_ranks(&path, &ctx)?;

    let fields = get_fields(&record.syntax());
    let sorted_fields = sorted_by_rank(&fields, |node| {
        *ranks.get(&get_field_name(node)).unwrap_or(&usize::max_value())
    });

    if sorted_fields == fields {
        return None;
    }

    ctx.add_assist(AssistId("reorder_fields"), "Reorder record fields", |edit| {
        for (old, new) in fields.iter().zip(&sorted_fields) {
            algo::diff(old, new).into_text_edit(edit.text_edit_builder());
        }
        edit.target(record.syntax().text_range())
    })
}

fn get_fields_kind(node: &SyntaxNode) -> Vec<SyntaxKind> {
    use SyntaxKind::*;
    match node.kind() {
        RECORD_LIT => vec![RECORD_FIELD],
        RECORD_PAT => vec![RECORD_FIELD_PAT, BIND_PAT],
        _ => vec![],
    }
}

fn get_field_name(node: &SyntaxNode) -> String {
    use SyntaxKind::*;
    match node.kind() {
        RECORD_FIELD => {
            if let Some(name) = node.children().find_map(NameRef::cast) {
                return name.to_string();
            }
            node.children().find_map(Expr::cast).map(|expr| expr.to_string()).unwrap_or_default()
        }
        BIND_PAT | RECORD_FIELD_PAT => {
            node.children().find_map(Name::cast).map(|n| n.to_string()).unwrap_or_default()
        }
        _ => String::new(),
    }
}

fn get_fields(record: &SyntaxNode) -> Vec<SyntaxNode> {
    let kinds = get_fields_kind(record);
    record.children().flat_map(|n| n.children()).filter(|n| kinds.contains(&n.kind())).collect()
}

fn sorted_by_rank(
    fields: &[SyntaxNode],
    get_rank: impl Fn(&SyntaxNode) -> usize,
) -> Vec<SyntaxNode> {
    fields.iter().cloned().sorted_by_key(get_rank).collect()
}

fn struct_definition(path: &ast::Path, sema: &Semantics<RootDatabase>) -> Option<Struct> {
    match sema.resolve_path(path) {
        Some(PathResolution::Def(ModuleDef::Adt(Adt::Struct(s)))) => Some(s),
        _ => None,
    }
}

fn compute_fields_ranks(path: &Path, ctx: &AssistCtx) -> Option<HashMap<String, usize>> {
    Some(
        struct_definition(path, ctx.sema)?
            .fields(ctx.db)
            .iter()
            .enumerate()
            .map(|(idx, field)| (field.name(ctx.db).to_string(), idx))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use crate::helpers::{check_assist, check_assist_not_applicable};

    use super::*;

    #[test]
    fn not_applicable_if_sorted() {
        check_assist_not_applicable(
            reorder_fields,
            r#"
        struct Foo {
            foo: i32,
            bar: i32,
        }

        const test: Foo = <|>Foo { foo: 0, bar: 0 };
        "#,
        )
    }

    #[test]
    fn trivial_empty_fields() {
        check_assist_not_applicable(
            reorder_fields,
            r#"
        struct Foo {};
        const test: Foo = <|>Foo {}
        "#,
        )
    }

    #[test]
    fn reorder_struct_fields() {
        check_assist(
            reorder_fields,
            r#"
        struct Foo {foo: i32, bar: i32};
        const test: Foo = <|>Foo {bar: 0, foo: 1}
        "#,
            r#"
        struct Foo {foo: i32, bar: i32};
        const test: Foo = <|>Foo {foo: 1, bar: 0}
        "#,
        )
    }

    #[test]
    fn reorder_struct_pattern() {
        check_assist(
            reorder_fields,
            r#"
        struct Foo { foo: i64, bar: i64, baz: i64 }

        fn f(f: Foo) -> {
            match f {
                <|>Foo { baz: 0, ref mut bar, .. } => (),
                _ => ()
            }
        }
        "#,
            r#"
        struct Foo { foo: i64, bar: i64, baz: i64 }

        fn f(f: Foo) -> {
            match f {
                <|>Foo { ref mut bar, baz: 0, .. } => (),
                _ => ()
            }
        }
        "#,
        )
    }

    #[test]
    fn reorder_with_extra_field() {
        check_assist(
            reorder_fields,
            r#"
            struct Foo {
                foo: String,
                bar: String,
            }

            impl Foo {
                fn new() -> Foo {
                    let foo = String::new();
                    <|>Foo {
                        bar: foo.clone(),
                        extra: "Extra field",
                        foo,
                    }
                }
            }
            "#,
            r#"
            struct Foo {
                foo: String,
                bar: String,
            }

            impl Foo {
                fn new() -> Foo {
                    let foo = String::new();
                    <|>Foo {
                        foo,
                        bar: foo.clone(),
                        extra: "Extra field",
                    }
                }
            }
            "#,
        )
    }
}
