//! 集成测试：策略沿用 todo_api_pg/tests/api.rs——不监听端口，用 tower 的
//! `oneshot` 把 Request 直接喂给 `app()` 返回的 Router，走完
//! 提取器(AuthUser)→限流→handler→services→错误转换的全链路。
//!
//! 与 pg 版相同的约束：所有测试连的是 docker-compose 里同一个 Postgres/Redis，
//! 因此全部标 `#[ignore]`（服务没起时 `cargo test` 依然全绿），断言容忍历史
//! 数据——todos 相关只认自己创建的 id/title。
//!
//! SaaS 版新增的约束：配额测试要「数 acme 的总条数」，与其他测试在同一租户上
//! 创建数据会互相干扰，所以：
//! 1. 所有测试通过一把进程级互斥锁串行执行（setup() 里拿 guard）；
//! 2. 会数数的测试开头调 reset_acme()：直接走库清空 acme 的 todos 并把 plan
//!    重置为 free，保证可重复运行；
//! 3. 不需要数数的测试尽量用 globex（pro，无配额）当创建方。
//!
//! 限流（30 次/分钟）没有专门的测试：要触发它得在一分钟内打 31 发请求，与
//! 「串行 + 可重复运行」的目标冲突（多跑几遍测试自己就先撞上限流了）。
//! setup() 里反而会清掉当前窗口的限流计数，把 429 从测试里隔离出去；
//! 限流行为留给手动验证：起服务后 `for i in $(seq 35); do curl ...; done`。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use redis::AsyncCommands;
use serde_json::{json, Value};
use sqlx::PgPool;
use todo_api_saas::config::Config;
use todo_api_saas::limits::{current_unix_minute, rate_limit_key};
use todo_api_saas::{app, build_state, signing, AppState};
use tokio::sync::{Mutex, MutexGuard};
use tower::ServiceExt;

/// 进程级互斥锁：cargo 默认并行跑测试，而这些测试共享同一个库和同一个租户，
/// 用锁串行化。用 tokio 的 Mutex 而非 std 的——guard 要跨 await 持有，
/// tokio::sync::Mutex 为此而生（也不会像 std 版那样被 panic 毒化）。
static LOCK: Mutex<()> = Mutex::const_new(());

struct TestCtx {
    _guard: MutexGuard<'static, ()>,
    app: Router,
    state: AppState,
}

/// 与生产同一套组装（build_state：连库+迁移+播种+连 Redis）。
/// 顺手清掉两租户当前/上一分钟的限流计数：反复跑测试累计的请求数
/// 不该让后面的断言收到意料之外的 429。
async fn setup() -> TestCtx {
    let guard = LOCK.lock().await;
    let state = build_state(Config::from_env())
        .await
        .expect("需要先 docker compose up -d postgres redis");
    let app = app(state.clone());
    let mut conn = state.redis.clone();
    for slug in ["acme", "globex"] {
        let tid = tenant_id(&state.pool, slug).await;
        let minute = current_unix_minute();
        for m in [minute, minute.saturating_sub(1)] {
            let _: i64 = conn.del(rate_limit_key(tid, m)).await.unwrap();
        }
    }
    TestCtx {
        _guard: guard,
        app,
        state,
    }
}

async fn tenant_id(pool: &PgPool, slug: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM saas.tenants WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await
        .expect("seeded tenant")
}

/// 让 acme 回到「free 套餐、0 条 todo」的初始态：配额/RBAC 测试的前置。
/// 直接走库而不是走 API——测试基建允许绕过业务规则（走 API 反而会被配额拦住）。
async fn reset_acme(pool: &PgPool) {
    sqlx::query(
        "DELETE FROM saas.todos WHERE tenant_id = (SELECT id FROM saas.tenants WHERE slug = 'acme')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE saas.tenants SET plan = 'free' WHERE slug = 'acme'")
        .execute(pool)
        .await
        .unwrap();
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!({}))
}

/// 随机 title：纳秒时间戳保证多次运行互不撞名，共享库里只认自己造的数据。
fn unique_title(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

/// 登录拿 token（种子密码统一 password123）。
async fn login(app: &Router, email: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "email": email, "password": "password123" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "login {email} failed");
    body_json(response).await["token"]
        .as_str()
        .expect("token in login response")
        .to_string()
}

/// 带 Bearer token 的请求构造器；body 为 Some 时按 JSON 发送。
fn authed(method: &str, uri: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

/// 造一条 todo，返回 id（默认断言 201）。
async fn create_todo(app: &Router, token: &str, title: &str) -> i64 {
    let response = app
        .clone()
        .oneshot(authed("POST", "/todos", token, Some(json!({ "title": title }))))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    body_json(response).await["id"].as_i64().unwrap()
}

// ---------- 1. 多租户隔离（红线测试） ----------
// 创建方用 globex（pro，无配额干扰），验证 acme 既列不到、也按 id 摸不到。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn tenant_isolation() {
    let ctx = setup().await;
    let globex_token = login(&ctx.app, "admin@globex.test").await;
    let acme_token = login(&ctx.app, "admin@acme.test").await;

    let title = unique_title("iso");
    let id = create_todo(&ctx.app, &globex_token, &title).await;

    // 创建方自己：列表可见、按 id 可取。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("GET", "/todos", &globex_token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let list = body_json(response).await;
    assert!(list.as_array().unwrap().iter().any(|t| t["id"] == id));

    // 另一个租户：列表看不到这条。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("GET", "/todos", &acme_token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let list = body_json(response).await;
    assert!(!list.as_array().unwrap().iter().any(|t| t["id"] == id));

    // 拿着别家的 id 直接访问：404——不是 403，不泄露「存在但不属于你」。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("GET", &format!("/todos/{id}"), &acme_token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------- 2. RBAC：删除仅 admin ----------
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn rbac_delete_admin_only() {
    let ctx = setup().await;
    reset_acme(&ctx.state.pool).await; // acme 回到 free/0 条，创建不会撞配额
    let admin_token = login(&ctx.app, "admin@acme.test").await;
    let member_token = login(&ctx.app, "member@acme.test").await;

    let id = create_todo(&ctx.app, &admin_token, &unique_title("rbac")).await;

    // member：同租户看得见（403 而非 404），但删不动。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("DELETE", &format!("/todos/{id}"), &member_token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // admin：204。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("DELETE", &format!("/todos/{id}"), &admin_token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

// ---------- 3. 配额 + webhook 升级即刻放开 ----------
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn quota_and_webhook_upgrade() {
    let ctx = setup().await;
    reset_acme(&ctx.state.pool).await;
    let token = login(&ctx.app, "admin@acme.test").await;

    // free 套餐：前 5 条都能建。
    for i in 0..5 {
        create_todo(&ctx.app, &token, &unique_title(&format!("quota{i}"))).await;
    }

    // 第 6 条：403 + 引导升级的错误消息。
    let response = ctx
        .app
        .clone()
        .oneshot(authed(
            "POST",
            "/todos",
            &token,
            Some(json!({ "title": unique_title("quota-over") })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = body_json(response).await;
    assert!(
        body["error"].as_str().unwrap().contains("quota"),
        "unexpected error body: {body}"
    );

    // 发一条验签正确的计费 webhook：acme 升 pro。
    let event = json!({
        "event": "subscription.updated",
        "tenant_slug": "acme",
        "plan": "pro"
    })
    .to_string();
    let signature = signing::sign_hex(ctx.state.config.webhook_secret.as_bytes(), event.as_bytes());
    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/billing")
                .header("content-type", "application/json")
                .header("x-signature", signature)
                .body(Body::from(event))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 配额即刻放开：plan 是每次 create 现查的，不存在缓存/token 延迟。
    create_todo(&ctx.app, &token, &unique_title("post-upgrade")).await;
}

// ---------- 4. 幂等键：重复 POST 只落库一条 ----------
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn idempotency_key_dedupes_create() {
    let ctx = setup().await;
    // 用 globex（pro）创建，不占 acme 的配额。
    let token = login(&ctx.app, "admin@globex.test").await;
    let title = unique_title("idem");
    let key = unique_title("idem-key"); // 每次运行换新键，Redis 里 24h 的旧键不干扰

    let send = |body_title: String, idem_key: String| {
        let app = ctx.app.clone();
        let token = token.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/todos")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .header("idempotency-key", idem_key)
                    .body(Body::from(json!({ "title": body_title }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    // 第一次：真创建，201。
    let response = send(title.clone(), key.clone()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let first = body_json(response).await;
    let first_id = first["id"].as_i64().unwrap();

    // 第二次（模拟客户端超时重试）：命中缓存，200，同一个 id。
    let response = send(title.clone(), key.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let second = body_json(response).await;
    assert_eq!(second["id"].as_i64().unwrap(), first_id);
    assert_eq!(second["title"], first["title"]);

    // 库里只有一条：幂等的最终判据在数据库，不在响应。
    let gid = tenant_id(&ctx.state.pool, "globex").await;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM saas.todos WHERE tenant_id = $1 AND title = $2")
            .bind(gid)
            .bind(&title)
            .fetch_one(&ctx.state.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

// ---------- 5. webhook 验签失败 ----------
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn webhook_bad_signature_rejected() {
    let ctx = setup().await;
    let event = json!({
        "event": "subscription.updated",
        "tenant_slug": "acme",
        "plan": "pro"
    })
    .to_string();

    // 签名是对「另一个 body」算的（等价于 body 被篡改）→ 401。
    let wrong_signature =
        signing::sign_hex(ctx.state.config.webhook_secret.as_bytes(), b"other-body");
    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/billing")
                .header("content-type", "application/json")
                .header("x-signature", wrong_signature)
                .body(Body::from(event.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 干脆不带签名头 → 401。
    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/billing")
                .header("content-type", "application/json")
                .body(Body::from(event))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------- 6. 无 token / 坏 token → 401 ----------
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn missing_or_bad_token_rejected() {
    let ctx = setup().await;

    // 无 Authorization 头。
    let response = ctx
        .app
        .clone()
        .oneshot(Request::builder().uri("/todos").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 坏 token（不是我们签的）。
    let response = ctx
        .app
        .clone()
        .oneshot(authed("GET", "/todos", "not-a-real-token", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
