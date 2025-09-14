use crate::{
    value::{Excerpt as _, Value},
    wqerr::{WqErr, WqErrType},
};

pub fn expected_numeric1(v: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg("expected int, bigint or float")
        .got1(v)
}

pub fn expected_numeric2(lhs: &Value, rhs: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg("expected int, bigint or float")
        .got2(lhs, rhs)
}

pub fn expected_integer1(v: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg("expected int or bigint")
        .got1(v)
}

pub fn expected_integer2(lhs: &Value, rhs: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg("expected int or bigint")
        .got2(lhs, rhs)
}

pub fn expected_bool1(v: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain).msg("expected bool").got1(v)
}

pub fn expected_bool2(lhs: &Value, rhs: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg("expected bool")
        .got2(lhs, rhs)
}

impl WqErr {
    pub fn got1(mut self, v: &Value) -> Self {
        self = self.attach_note(format!("got '{}' ({})", v.excerpt(), v.type_name()));
        self
    }

    pub fn got2(mut self, lhs: &Value, rhs: &Value) -> Self {
        self = self.attach_note(format!("got lhs '{}' ({})", lhs.excerpt(), lhs.type_name()));
        self = self.attach_note(format!("got rhs '{}' ({})", rhs.excerpt(), rhs.type_name()));
        self
    }

    pub fn offending_elem(mut self, v: &Value, i: usize) -> Self {
        self = self.attach_note(format!(
            "got offending element '{}' ({}) at [{i}]",
            v.excerpt(),
            v.type_name()
        ));
        self
    }
}
