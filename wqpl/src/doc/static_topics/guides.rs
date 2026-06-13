use super::super::model::{DocExample, DocKind, ExampleExpectation, StaticDoc};

const OPERATOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Operators are functions too",
    code: "+[1;2;3]",
    expectation: ExampleExpectation::ResultContains("6"),
}];

pub(super) const BUILTINS: StaticDoc = StaticDoc {
    id: "builtins",
    title: "Builtins",
    kind: DocKind::Guide,
    group: "Reference",
    aliases: &["bfn", "builtin", "builtins"],
    summary: "Built-in functions are values provided by wq.",
    details: "Builtins can be called with bracket syntax, postfix syntax for one argument, or through pipes. Individual builtin pages always render their signature and arity from `builtins.rs` metadata.",
    examples: &[],
    related: &["operators", "calls"],
};

pub(super) const OPERATORS: StaticDoc = StaticDoc {
    id: "operators",
    title: "Operators",
    kind: DocKind::Guide,
    group: "Reference",
    aliases: &["operator", "operators", "+", "-", "*", "/", ","],
    summary: "Operators are also builtin functions.",
    details: "Most binary operators broadcast over compatible values. The comma operator concatenates, while leading comma enlists a value.",
    examples: OPERATOR_EXAMPLES,
    related: &["builtins", "lists", "pipes"],
};
