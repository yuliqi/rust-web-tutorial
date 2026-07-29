//! 第 8 章：测试与质量

/// 规范化待办标题。
///
/// ```
/// assert_eq!(ch08_testing::create_todo_title("  hi  ").unwrap(), "hi");
/// ```
pub fn create_todo_title(raw: &str) -> Result<String, String> {
    let title = raw.trim();
    if title.is_empty() {
        return Err("title required".into());
    }
    if title.len() > 100 {
        return Err("title too long".into());
    }
    Ok(title.to_string())
}

/// 加法（文档测试示例）。
///
/// ```
/// assert_eq!(ch08_testing::add(1, 2), 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
}

#[derive(Default)]
pub struct MemoryRepo {
    next_id: u64,
    items: Vec<Todo>,
}

impl MemoryRepo {
    pub fn create(&mut self, title: &str) -> Result<Todo, String> {
        let title = create_todo_title(title)?;
        self.next_id += 1;
        let todo = Todo {
            id: self.next_id,
            title,
        };
        self.items.push(todo.clone());
        Ok(todo)
    }

    pub fn get(&self, id: u64) -> Option<&Todo> {
        self.items.iter().find(|t| t.id == id)
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|t| t.id != id);
        before != self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_ok() {
        assert_eq!(create_todo_title("  task  ").unwrap(), "task");
    }

    #[test]
    fn title_empty() {
        assert_eq!(create_todo_title("   ").unwrap_err(), "title required");
    }

    #[test]
    fn title_too_long() {
        let s = "a".repeat(101);
        assert_eq!(create_todo_title(&s).unwrap_err(), "title too long");
    }

    #[test]
    fn repo_crud() {
        let mut repo = MemoryRepo::default();
        let t = repo.create("one").unwrap();
        assert_eq!(repo.get(t.id).map(|x| x.title.as_str()), Some("one"));
        assert!(repo.delete(t.id));
        assert!(repo.get(t.id).is_none());
    }
}
