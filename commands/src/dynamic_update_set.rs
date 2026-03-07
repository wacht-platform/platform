pub(crate) struct DynamicUpdateSet {
    set_parts: Vec<String>,
    next_param: usize,
}

impl DynamicUpdateSet {
    pub(crate) fn with_updated_at() -> Self {
        Self {
            set_parts: vec!["updated_at = $1".to_string()],
            next_param: 2,
        }
    }

    pub(crate) fn push_if_present<T>(&mut self, column: &str, value: &Option<T>) {
        if value.is_some() {
            self.push(column);
        }
    }

    fn push(&mut self, column: &str) {
        self.set_parts
            .push(format!("{column} = ${}", self.next_param));
        self.next_param += 1;
    }

    pub(crate) fn set_clause(&self) -> String {
        self.set_parts.join(", ")
    }

    pub(crate) fn where_indexes(&self) -> (usize, usize) {
        (self.next_param, self.next_param + 1)
    }
}
