//! Todo 的 HTTP 层：只做「翻译」——把请求解析成类型化参数，调 services，再把结果包成响应。
//! 业务规则（校验、SQL）一概不在这里，所以每个 handler 都只有两三行。
//! 数据流：请求 → 本文件(提取器) → services/todos.rs → db。
//!
//! 提取器（Extractor）是 axum 的核心机制：handler 参数列表里的
//! State/Path/Query/Json 声明了「我要从请求里拿什么」，解析失败框架自动返 4xx。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::error::AppResult;
use crate::models::{CreateTodo, ListQuery, Todo, UpdateTodo};
use crate::services::todos as todos_service;
use crate::AppState;

/// 同一路径的不同方法链式挂载（get(...).post(...)），RESTful 风格的标准写法。
/// `{id}` 是 axum 0.8 的路径参数语法，由 handler 里的 `Path<i64>` 接住。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/todos", get(list_todos).post(create_todo))
        .route(
            "/todos/{id}",
            get(get_todo).patch(update_todo).delete(delete_todo),
        )
}

// State(state) 拿到的是 AppState 的 clone；其中的 pool clone 开销极小（见 lib.rs）。
// 返回 AppResult：`?` 抛出的 AppError 会经 IntoResponse 变成对应的 HTTP 错误（见 error.rs）。
async fn list_todos(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<Todo>>> {
    let items = todos_service::list(&state.pool, query.done).await?;
    Ok(Json(items))
}

async fn get_todo(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Todo>> {
    let todo = todos_service::get(&state.pool, id).await?;
    Ok(Json(todo))
}

// Json<CreateTodo> 会消费请求体，因此按 axum 的规则必须放在参数列表最后。
// 用元组 (StatusCode, Json<T>) 定制状态码：创建成功按 REST 惯例返回 201 而非默认 200。
async fn create_todo(
    State(state): State<AppState>,
    Json(payload): Json<CreateTodo>,
) -> AppResult<(StatusCode, Json<Todo>)> {
    let todo = todos_service::create(&state.pool, payload).await?;
    Ok((StatusCode::CREATED, Json(todo)))
}

async fn update_todo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateTodo>,
) -> AppResult<Json<Todo>> {
    let todo = todos_service::update(&state.pool, id, payload).await?;
    Ok(Json(todo))
}

// 删除成功没有响应体，直接返回 204 No Content。
async fn delete_todo(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    todos_service::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
