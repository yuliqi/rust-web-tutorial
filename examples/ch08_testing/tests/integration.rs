use ch08_testing::{create_todo_title, MemoryRepo};

#[test]
fn public_api_create_title() {
    assert!(create_todo_title("ok").is_ok());
}

#[test]
fn public_api_repo() {
    let mut repo = MemoryRepo::default();
    let todo = repo.create("integration").unwrap();
    assert_eq!(todo.id, 1);
}
