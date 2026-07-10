use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity_named};
use crate::session::stdio::wqstdout_println;
use crate::value::display::{TableFormatOptions, TableStyle, format_table_value_with_options};
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn show_table(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_named(
        BE::Showtable,
        [1],
        &args,
        super::super::SHOWTABLE_NAMED_ARGS,
    )?;
    let opts = parse_table_options(&args)?;
    let formatted = format_table_value_with_options(&args[0], &opts).map_err(|msg| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .msg(msg)
    })?;
    if let Some(table) = formatted {
        wqstdout_println(table);
        return Ok(Value::unit());
    }
    Err(WqError::new(WqErrorType::Domain).src(BE::Showtable).msg("invalid table, expected (a dict), (a list of dicts), (a dict of lists), or (a dict of dicts)"))
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
                    .msg("style must be \"plain\" or \"markdown\""));
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
            .msg("cols must name at least one column"))
    } else {
        Ok(names)
    }
}

fn parse_text(value: &Value, option: &str) -> WqResult<String> {
    match value {
        Value::Tag(sym) => Ok(sym.to_string()),
        _ => value.to_rust_string_with_note().map_err(|e| {
            e.src(BE::Showtable)
                .msg(format!("{option} must be a string, char, or tag"))
        }),
    }
}

fn parse_nonnegative_usize(value: &Value, option: &str) -> WqResult<usize> {
    let Some(number) = value.as_i64() else {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .msg(format!("{option} must be a nonnegative integer")));
    };
    if number < 0 {
        Err(WqError::new(WqErrorType::Domain)
            .src(BE::Showtable)
            .msg(format!("{option} must be a nonnegative integer")))
    } else {
        usize::try_from(number).map_err(|_| {
            WqError::new(WqErrorType::Domain)
                .src(BE::Showtable)
                .msg(format!("{option} is too large"))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::*;
    use crate::value::display::format_table_value;

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
            "table column `age` was not found"
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
