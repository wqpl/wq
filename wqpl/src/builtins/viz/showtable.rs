use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::session::stdio::wqstdout_println;
use crate::value::display::format_table_value;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn show_table(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Showtable, [1], &args)?;
    if let Some(table) = format_table_value(&args[0]) {
        wqstdout_println(table);
        return Ok(Value::unit());
    }
    Err(WqError::new(WqErrorType::Domain).src(BE::Showtable).msg("invalid table, expected (a dict), (a list of dicts), (a dict of lists), or (a dict of dicts)"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::*;

    #[test]
    fn formats_dict_of_scalars_as_single_row_table() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("name".into(), Value::Tag("ada".into())),
            ("age".into(), Value::Int(37)),
        ])));

        assert_eq!(
            format_table_value(&value).as_deref(),
            Some("name age\n`ada 37")
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
            Some("name   age\n`ada\n`grace 85")
        );
    }

    #[test]
    fn rejects_non_table_values() {
        assert_eq!(format_table_value(&Value::Int(1)), None);
    }
}
