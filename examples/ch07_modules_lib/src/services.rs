use crate::models::Todo;
use std::collections::HashMap;

#[derive(Default)]
pub struct MemoryTodoRepo {
    data: HashMap<u64, Todo>,
}

impl MemoryTodoRepo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, todo: Todo) {
        self.data.insert(todo.id, todo);
    }

    pub fn get(&self, id: u64) -> Option<&Todo> {
        self.data.get(&id)
    }

    pub fn list(&self) -> Vec<Todo> {
        let mut items: Vec<_> = self.data.values().cloned().collect();
        items.sort_by_key(|t| t.id);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut repo = MemoryTodoRepo::new();
        repo.insert(Todo::new(1, "modular"));
        assert_eq!(repo.get(1).map(|t| t.title.as_str()), Some("modular"));
    }
}
