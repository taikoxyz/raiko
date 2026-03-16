use axum::{
    extract::Path,
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};

use crate::{models::CreateTicketRequest, AppState};

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/tickets", post(create_ticket))
        .route("/api/tickets/:ticket_id", get(get_ticket))
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html>
  <body>
    <h1>Mock Studio</h1>
    <form method="post" action="/api/tickets">
      <textarea name="requirement" rows="8" cols="80"></textarea>
      <button type="submit">Create Ticket</button>
    </form>
  </body>
</html>"#,
    )
}

async fn create_ticket(
    State(state): State<AppState>,
    Json(request): Json<CreateTicketRequest>,
) -> impl IntoResponse {
    let ticket = state.service.submit_ticket(&request.requirement).await;
    Json(ticket)
}

async fn get_ticket(
    State(state): State<AppState>,
    Path(ticket_id): Path<String>,
) -> impl IntoResponse {
    Json(state.service.get_ticket(&ticket_id))
}
