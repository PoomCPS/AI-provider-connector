use axum::response::Html;

const DASHBOARD_HTML: &str = include_str!("../dashboard/index.html");

/// Handler for GET /dashboard
pub async fn dashboard_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}
