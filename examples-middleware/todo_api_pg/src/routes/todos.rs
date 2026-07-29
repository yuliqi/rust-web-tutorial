//! Todo 的 HTTP 层：只做「翻译」——解析请求、调 services、包装响应。
//! 与 SQLite 版逐字相同：方言差异被 services/db 层完全吸收，HTTP 层无感。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::error::AppResult;
use crate::models::{CreateTodo, ListQuery, Todo, UpdateTodo};
use crate::services::todos as todos_service;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/todos", get(list_todos).post(create_todo))
        .route(
            "/todos/{id}",
            get(get_todo).patch(update_todo).delete(delete_todo),
        )
}

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

// Json<CreateTodo> 消费请求体，必须放参数列表最后；创建成功按 REST 惯例返回 201。
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

async fn delete_todo(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    todos_service::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
