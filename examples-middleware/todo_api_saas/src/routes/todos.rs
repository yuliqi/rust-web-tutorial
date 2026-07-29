//! Todo 的 HTTP 层。对比 todo_api_pg，每个 handler 的参数列表里多了
//! `user: AuthUser`——这一行就是「本路由需要登录」的全部声明（见 auth.rs）。
//! handler 里再把 user.tenant_id 传给 services：HTTP 层负责「你是谁」，
//! services 层负责「你只能看你自己的」。
//!
//! 限流（第 18 章）：每个 handler 开头调 `enforce_rate_limit`。也可以写成
//! tower 中间件挂在整棵子路由上，但限流键需要 tenant_id、而 tenant_id 要解完
//! token 才有——用「handler 前置调用」能直接复用 AuthUser 的解析结果，
//! 代价是每个 handler 手写一行（忘写就是漏洞，中间件版留作练习）。

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use redis::AsyncCommands;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::limits;
use crate::models::{CreateTodo, Todo};
use crate::services::todos as todos_service;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/todos", get(list_todos).post(create_todo))
        .route("/todos/{id}", get(get_todo).delete(delete_todo))
}

async fn list_todos(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<Todo>>> {
    limits::enforce_rate_limit(&state.redis, user.tenant_id).await?;
    // tenant_id 从 AuthUser（即 JWT Claims）取——安全红线，见 services/todos.rs。
    let items = todos_service::list(&state.pool, user.tenant_id).await?;
    Ok(Json(items))
}

async fn get_todo(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<Todo>> {
    limits::enforce_rate_limit(&state.redis, user.tenant_id).await?;
    let todo = todos_service::get(&state.pool, user.tenant_id, id).await?;
    Ok(Json(todo))
}

/// 创建，支持可选的 `Idempotency-Key` 头（第 18 章）。
///
/// 为什么需要幂等键：客户端发出 POST 后网络超时，它不知道服务端到底建没建成，
/// 只能重试——POST 不幂等，重试就是重复下单/重复扣款，这是真实世界最常见的
/// 资损事故来源之一。行业标准解法（Stripe 等支付 API 均如此）：客户端为每次
/// 「业务意图」生成一个唯一键随请求携带；服务端第一次处理后把响应存下，
/// 相同键的重试直接返回**当初存下的那份响应**，绝不重复执行。
async fn create_todo(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(payload): Json<CreateTodo>,
) -> AppResult<Response> {
    limits::enforce_rate_limit(&state.redis, user.tenant_id).await?;

    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut conn = state.redis.clone();

    // 命中已有键：返回缓存的响应体，状态码用 200（不是 201——资源不是这次
    // 请求创建的）。客户端拿到的 JSON 与第一次逐字节相同，重试对它完全透明。
    if let Some(key) = &idem_key {
        let redis_key = limits::idempotency_redis_key(user.tenant_id, key);
        let cached: Option<String> = conn.get(&redis_key).await?;
        if let Some(body) = cached {
            return Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response());
        }
    }

    let todo = todos_service::create(&state.pool, user.tenant_id, payload).await?;

    if let Some(key) = &idem_key {
        let redis_key = limits::idempotency_redis_key(user.tenant_id, key);
        let body = serde_json::to_string(&todo).map_err(anyhow::Error::from)?;
        // NX：只有键不存在时才写入（并发重试也只有一个赢家）；EX 86400：存 24h。
        // 已知竞态：GET 与 SET 之间的窗口里，两个并发的同键请求可能都走到
        // create——严格解法是先 SET NX 占位、后写结果（或加锁），教学从简。
        let _: Option<String> = redis::cmd("SET")
            .arg(&redis_key)
            .arg(&body)
            .arg("NX")
            .arg("EX")
            .arg(limits::IDEMPOTENCY_TTL_SECS)
            .query_async(&mut conn)
            .await?;
    }

    Ok((StatusCode::CREATED, Json(todo)).into_response())
}

/// 删除：RBAC——仅 admin（第 17 章）。member 拿到 403（不是 404：它有权
/// 知道这条 todo 存在——列表里看得见——只是没权删；跨租户才是 404）。
///
/// role 放 JWT 里的取舍：省掉每个请求查一次 users 表（无状态、可水平扩展），
/// 代价是「撤销延迟」——把某人从 admin 降为 member 后，他手里的旧 token
/// 在过期前（最长 1 小时）依然带着 admin。对多数业务这可以接受；对撤销
/// 必须即刻生效的场景（封号、安全事件），就得回到「每请求查库」或引入
/// token 版本号/黑名单——用状态换实时性。
async fn delete_todo(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    limits::enforce_rate_limit(&state.redis, user.tenant_id).await?;
    if !user.is_admin() {
        return Err(AppError::forbidden("admin role required to delete todos"));
    }
    todos_service::delete(&state.pool, user.tenant_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
