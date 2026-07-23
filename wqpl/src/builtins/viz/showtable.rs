use crate::builtins::{BuiltinContext, BuiltinEnum as BE, BuiltinFnArgs, check_registered_args};
use crate::value::display::{TableFormatOptions, TableStyle, format_table_value_with_options};
use crate::value::{Value, WqResult};
use crate::wqerror::{Bound, Requirement, WqError, WqErrorType};

pub(crate) fn show_table(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BE::Showtable, &args)?;
    let opts = parse_table_options(&args)?;
    let formatted = format_table_value_with_options(&args[0], &opts).map_err(|msg| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .msg(msg)
    })?;
    if let Some(table) = formatted {
        vm.write_stdout_line(&table).map_err(|error| {
            WqError::new(WqErrorType::Io)
                .src(BE::Showtable)
                .attach_note(format!("host I/O error: {error}"))
        })?;
        return Ok(Value::empty_list());
    }
    Err(WqError::new(WqErrorType::Domain)
        .src(BE::Showtable)
        .msg("expected a dict, a list of dicts, a dict of lists, or a dict of dicts"))
}

fn parse_table_options(args: &BuiltinFnArgs) -> WqResult<TableFormatOptions> {
    let mut opts = TableFormatOptions::default();
    if let Some(value) = args.named("cols") {
        opts.columns = Some(parse_column_names(value)?);
    }
    if let Some(value) = args.named("limit") {
        opts.limit = Some(parse_nonnegative_usize(value, "limit")?);
    }
    if let Some(value) = args.named("width") {
        let width = parse_nonnegative_usize(value, "width")?;
        opts.max_cell_width = Some(width);
    }
    if let Some(value) = args.named("missing") {
        opts.missing = parse_text(value, "missing")?;
    }
    if let Some(value) = args.named("style") {
        let style = parse_text(value, "style")?;
        opts.style = match style.as_str() {
            "plain" => TableStyle::Plain,
            "markdown" | "md" => TableStyle::Markdown,
            _ => {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Showtable)
                    .expected(Requirement::one_of([
                        Requirement::string_literal("plain"),
                        Requirement::string_literal("markdown"),
                        Requirement::string_literal("md"),
                    ]))
                    .at_named_arg("style")
                    .got1(value));
            }
        };
    }
    Ok(opts)
}

fn parse_column_names(value: &Value) -> WqResult<Vec<String>> {
    let names = match value {
        Value::List(items) if !items.iter().all(|item| matches!(item, Value::Char(_))) => {
            let mut names = Vec::new();
            for item in items.iter() {
                names.push(parse_text(item, "cols")?);
            }
            names
        }
        _ => vec![parse_text(value, "cols")?],
    };
    if names.is_empty() || names.iter().any(|name| name.is_empty()) {
        Err(WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .expected(Requirement::phrase(
                "at least one non-empty column name",
                "non-empty column names",
            ))
            .at_named_arg("cols")
            .got1(value))
    } else {
        Ok(names)
    }
}

fn parse_text(value: &Value, option: &str) -> WqResult<String> {
    match value {
        Value::Tag(sym) => Ok(sym.to_string()),
        _ => value.try_to_rust_string().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BE::Showtable)
                .expected(Requirement::one_of([
                    Requirement::STRING,
                    Requirement::CHAR,
                    Requirement::TAG,
                ]))
                .at_named_arg(option)
                .got1(value)
        }),
    }
}

fn parse_nonnegative_usize(value: &Value, option: &str) -> WqResult<usize> {
    let error = || {
        let max = i128::try_from(usize::MAX)
            .expect("usize fits in i128")
            .min(i128::from(i64::MAX));
        WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .expected(Requirement::int_range(
                Bound::Included(0),
                Bound::Included(max),
            ))
            .at_named_arg(option)
            .got1(value)
    };
    let Some(number) = value.as_i64() else {
        return Err(error());
    };
    if number < 0 {
        Err(error())
    } else {
        usize::try_from(number).map_err(|_| error())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::*;
    use crate::value::display::format_table_value;
    use crate::vm::Vm;

    #[test]
    fn diagnostics_use_canonical_table_and_option_wording() {
        let mut vm = Vm::new(Vec::new());
        let table_error = show_table(&mut vm, BuiltinFnArgs::from(Value::Int(1)))
            .expect_err("an int is not table-shaped");
        assert_eq!(
            table_error.msg.as_deref(),
            Some("expected a dict, a list of dicts, a dict of lists, or a dict of dicts")
        );

        let limit_error = parse_nonnegative_usize(&Value::Int(-1), "limit")
            .expect_err("negative limit should fail");
        assert_eq!(
            limit_error.msg.as_deref(),
            Some(if usize::BITS >= 64 {
                "expected int from 0 through 9223372036854775807"
            } else {
                "expected int from 0 through 4294967295"
            })
        );
        assert_eq!(
            limit_error.notes.as_ref(),
            &["at named argument 'limit'", "got -1 (int)"]
        );

        let style_error = parse_table_options(&BuiltinFnArgs::with_named(
            smallvec::smallvec![],
            vec![(Arc::from("style"), Value::String(Arc::new("csv".into())))],
        ))
        .expect_err("unknown style should fail");
        assert_eq!(
            style_error.msg.as_deref(),
            Some("expected \"plain\", \"markdown\", or \"md\"")
        );
        assert_eq!(
            style_error.notes.as_ref(),
            &["at named argument 'style'", "got \"csv\" (list)"]
        );
    }

    #[test]
    fn formats_dict_of_atoms_as_single_row_table() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("name".into(), Value::Tag("ada".into())),
            ("age".into(), Value::Int(37)),
        ])));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("name age\n`ada  37")
        );
    }

    #[test]
    fn formats_char_lists_as_text_cells() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            (
                "name".into(),
                Value::List(Arc::new(vec![
                    Value::Char('a'),
                    Value::Char('d'),
                    Value::Char('a'),
                ])),
            ),
            ("age".into(), Value::Int(37)),
        ])));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("name  age\n\"ada\"  37")
        );
    }

    #[test]
    fn formats_dict_of_dicts_with_row_column() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            (
                "laptop".into(),
                Value::Dict(Arc::new(IndexMap::from([
                    ("price".into(), Value::Int(1200)),
                    ("qty".into(), Value::Int(10)),
                ]))),
            ),
            (
                "mouse".into(),
                Value::Dict(Arc::new(IndexMap::from([
                    ("price".into(), Value::Int(25)),
                    ("qty".into(), Value::Int(100)),
                ]))),
            ),
        ])));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("row    price qty\nlaptop  1200  10\nmouse     25 100")
        );
    }

    #[test]
    fn formats_list_of_dicts_with_sparse_columns() {
        let value = Value::List(Arc::new(vec![
            Value::Dict(Arc::new(IndexMap::from([(
                "name".into(),
                Value::Tag("ada".into()),
            )]))),
            Value::Dict(Arc::new(IndexMap::from([
                ("name".into(), Value::Tag("grace".into())),
                ("age".into(), Value::Int(85)),
            ]))),
        ]));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("name   age\n`ada\n`grace  85")
        );
    }

    #[test]
    fn formats_selected_and_limited_columns() {
        let value = Value::List(Arc::new(vec![
            Value::Dict(Arc::new(IndexMap::from([
                ("name".into(), Value::Tag("ada".into())),
                ("age".into(), Value::Int(37)),
                ("dept".into(), Value::Tag("math".into())),
            ]))),
            Value::Dict(Arc::new(IndexMap::from([
                ("name".into(), Value::Tag("grace".into())),
                ("age".into(), Value::Int(85)),
                ("dept".into(), Value::Tag("navy".into())),
            ]))),
        ]));
        let opts = TableFormatOptions {
            columns: Some(vec!["name".to_string(), "age".to_string()]),
            limit: Some(1),
            ..TableFormatOptions::default()
        };

        assert_eq!(
            format_table_value_with_options(&value, &opts)
                .unwrap()
                .as_deref(),
            Some("name age\n`ada  37\n... 1 more rows")
        );
    }

    #[test]
    fn formats_markdown_tables() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            (
                "name".into(),
                Value::List(Arc::new(vec![
                    Value::String(Arc::new("ada".to_string())),
                    Value::String(Arc::new("grace".to_string())),
                ])),
            ),
            ("age".into(), Value::IntList(Arc::new(vec![37, 85]))),
        ])));
        let opts = TableFormatOptions {
            style: TableStyle::Markdown,
            ..TableFormatOptions::default()
        };

        assert_eq!(
            format_table_value_with_options(&value, &opts)
                .unwrap()
                .as_deref(),
            Some("| name    | age |\n| ------- | --: |\n| \"ada\"   |  37 |\n| \"grace\" |  85 |")
        );
    }

    #[test]
    fn formats_virtual_range_columns() {
        let value = Value::Dict(Arc::new(IndexMap::from([(
            "n".into(),
            Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 3))),
        )])));
        assert_eq!(format_table_value(&value).as_deref(), Some("n\n1\n2\n3"));
    }

    #[test]
    fn truncates_cells_by_display_width() {
        let value = Value::Dict(Arc::new(IndexMap::from([(
            "note".into(),
            Value::String(Arc::new("display width".to_string())),
        )])));
        let opts = TableFormatOptions {
            max_cell_width: Some(7),
            ..TableFormatOptions::default()
        };

        assert_eq!(
            format_table_value_with_options(&value, &opts)
                .unwrap()
                .as_deref(),
            Some("note\n\"displ…")
        );
    }

    #[test]
    fn errors_on_missing_selected_column() {
        let value = Value::Dict(Arc::new(IndexMap::from([("name".into(), Value::Int(1))])));
        let opts = TableFormatOptions {
            columns: Some(vec!["age".to_string()]),
            ..TableFormatOptions::default()
        };

        assert_eq!(
            format_table_value_with_options(&value, &opts).unwrap_err(),
            "table column 'age' was not found"
        );
    }

    #[test]
    fn formats_unicode_with_display_width() {
        let value = Value::List(Arc::new(vec![
            Value::Dict(Arc::new(IndexMap::from([
                ("name".into(), Value::Tag("猫".into())),
                ("age".into(), Value::Int(7)),
            ]))),
            Value::Dict(Arc::new(IndexMap::from([
                ("name".into(), Value::Tag("ada".into())),
                ("age".into(), Value::Int(85)),
            ]))),
        ]));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("name age\n`猫    7\n`ada  85")
        );
    }

    #[test]
    fn rejects_non_table_values() {
        assert_eq!(format_table_value(&Value::Int(1)), None);
    }
}
