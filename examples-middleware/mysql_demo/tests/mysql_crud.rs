//! 集成测试：真连 MySQL 走一遍 CRUD。
//! 默认被 ignore——CI 或没起容器的机器上只编译不运行；
//! 本地想跑：`docker compose up -d mysql` 之后 `cargo test -p mysql_demo -- --ignored`。

use mysql_demo::{connect, create_todo, delete_todo, get_todo, list_todos, set_done};

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://root:tutorial@localhost:3306/todos".to_string())
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d mysql"]
async fn full_crud_roundtrip() {
    let pool = connect(&database_url()).await.expect("连接失败,请先 docker compose up -d mysql");

    // 库是共享的、可能已有历史数据,所以断言只围绕「本测试创建的这一条」展开,
    // 不假设表为空,也不假设 id 从 1 开始。
    let title = format!("integration-test-{}", std::process::id());
    let todo = create_todo(&pool, &title).await.unwrap();
    assert!(todo.id > 0);
    assert_eq!(todo.title, title);
    assert!(!todo.done);

    // 读回来应与插入时一致。
    let fetched = get_todo(&pool, todo.id).await.unwrap().expect("刚插入的行应能查到");
    assert_eq!(fetched.title, title);
    assert!(!fetched.done);

    // 列表里应包含这一条(容忍其它历史数据同时存在)。
    let all = list_todos(&pool).await.unwrap();
    assert!(all.iter().any(|t| t.id == todo.id));

    // 更新为完成,再读回验证。
    assert!(set_done(&pool, todo.id, true).await.unwrap());
    let done = get_todo(&pool, todo.id).await.unwrap().unwrap();
    assert!(done.done);

    // 删除后应查不到;重复删除返回 false。
    assert!(delete_todo(&pool, todo.id).await.unwrap());
    assert!(get_todo(&pool, todo.id).await.unwrap().is_none());
    assert!(!delete_todo(&pool, todo.id).await.unwrap());
}
