use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};

use crate::{
    models::{CreateTicketRequest, GatewayProxyRequest},
    AppState,
};

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Mock Studio</title>
    <style>
      :root {
        color-scheme: light;
        --bg: #f5f1e8;
        --panel: rgba(255, 255, 255, 0.78);
        --panel-border: rgba(31, 41, 55, 0.12);
        --text: #111827;
        --muted: #6b7280;
        --accent: #0f766e;
        --accent-strong: #134e4a;
        --shadow: 0 18px 45px rgba(15, 23, 42, 0.11);
      }
      * { box-sizing: border-box; }
      body {
        margin: 0;
        font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        color: var(--text);
        background:
          radial-gradient(circle at top left, rgba(15, 118, 110, 0.14), transparent 30%),
          radial-gradient(circle at top right, rgba(217, 119, 6, 0.12), transparent 28%),
          linear-gradient(180deg, #faf7f0 0%, var(--bg) 100%);
        min-height: 100vh;
      }
      .shell {
        max-width: 1320px;
        margin: 0 auto;
        padding: 24px;
      }
      .hero, .card, .output {
        background: var(--panel);
        border: 1px solid var(--panel-border);
        box-shadow: var(--shadow);
        backdrop-filter: blur(14px);
      }
      .hero {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        padding: 20px 24px;
        border-radius: 24px;
        margin-bottom: 18px;
      }
      .hero h1 {
        margin: 0;
        font-size: clamp(28px, 4vw, 44px);
        letter-spacing: -0.04em;
      }
      .hero p {
        margin: 8px 0 0;
        color: var(--muted);
      }
      .login-pill {
        display: inline-flex;
        align-items: center;
        gap: 10px;
        padding: 10px 14px;
        border-radius: 999px;
        border: 1px solid rgba(15, 23, 42, 0.1);
        background: rgba(255, 255, 255, 0.76);
        color: var(--accent-strong);
      }
      .login-pill svg { width: 18px; height: 18px; }
      .grid {
        display: grid;
        grid-template-columns: minmax(320px, 1.05fr) minmax(360px, 1fr);
        gap: 18px;
      }
      .card, .output {
        border-radius: 24px;
        padding: 20px;
      }
      .card h2, .output h2 {
        margin: 0 0 14px;
        font-size: 18px;
        letter-spacing: -0.02em;
      }
      .field {
        margin-bottom: 14px;
      }
      label {
        display: block;
        font-size: 13px;
        font-weight: 700;
        color: var(--muted);
        margin-bottom: 8px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
      }
      select, textarea, input {
        width: 100%;
        border-radius: 16px;
        border: 1px solid rgba(15, 23, 42, 0.14);
        background: rgba(255, 255, 255, 0.9);
        color: var(--text);
        font: inherit;
        padding: 12px 14px;
      }
      textarea {
        min-height: 124px;
        resize: vertical;
      }
      textarea[readonly] {
        min-height: 160px;
        background: rgba(248, 250, 252, 0.92);
      }
      .row {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 12px;
      }
      .meta {
        display: grid;
        gap: 8px;
        padding: 14px;
        border-radius: 18px;
        background: rgba(255, 255, 255, 0.72);
        border: 1px solid rgba(15, 23, 42, 0.08);
        margin-bottom: 14px;
      }
      .meta div {
        display: flex;
        justify-content: space-between;
        gap: 16px;
        font-size: 14px;
      }
      .meta span:first-child { color: var(--muted); }
      .status {
        font-weight: 700;
        color: var(--accent-strong);
      }
      .actions {
        display: flex;
        gap: 10px;
        flex-wrap: wrap;
        margin-top: 8px;
      }
      button {
        border: 0;
        border-radius: 999px;
        padding: 12px 16px;
        font-weight: 700;
        cursor: pointer;
      }
      .primary {
        background: linear-gradient(135deg, var(--accent), #115e59);
        color: white;
      }
      .secondary {
        background: rgba(15, 23, 42, 0.06);
        color: var(--text);
      }
      button:disabled,
      textarea:disabled,
      select:disabled {
        opacity: 0.52;
        cursor: not-allowed;
      }
      .hint {
        color: var(--muted);
        font-size: 13px;
        margin-top: 8px;
      }
      .output pre {
        margin: 0;
        white-space: pre-wrap;
        word-break: break-word;
      }
      @media (max-width: 920px) {
        .grid { grid-template-columns: 1fr; }
        .hero { flex-direction: column; align-items: flex-start; }
        .row { grid-template-columns: 1fr; }
      }
    </style>
  </head>
  <body>
    <div class="shell">
      <header class="hero">
        <div>
          <h1>Mock Studio</h1>
          <p>Submit a ticket, watch the gateway come up, then probe the raw JSON response.</p>
        </div>
        <button class="login-pill" type="button" aria-label="Login placeholder" title="Login placeholder">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="8" r="3.5"></circle>
            <path d="M5 20c1.7-3.4 5.3-5.5 7-5.5s5.3 2.1 7 5.5"></path>
          </svg>
          Login
        </button>
      </header>

      <main class="grid">
        <section class="card">
          <h2>Tickets</h2>
          <div class="field">
            <label for="ticket-history">History</label>
            <select id="ticket-history"></select>
          </div>
          <div class="field">
            <label for="ticket-requirement">Requirement</label>
            <textarea id="ticket-requirement" placeholder="Type a ticket requirement here"></textarea>
          </div>
          <div class="actions">
            <button class="primary" id="submit-ticket" type="button">Submit Ticket</button>
            <button class="secondary" id="reload-state" type="button">Refresh</button>
          </div>
          <div class="meta">
            <div><span>Ticket ID</span><span id="ticket-id">-</span></div>
            <div><span>Rule ID</span><span id="rule-id">-</span></div>
            <div><span>Status</span><span class="status" id="ticket-status">-</span></div>
            <div><span>Gateway Runtime</span><span id="ticket-runtime">-</span></div>
            <div><span>Handler Mode</span><span id="ticket-handler-mode">-</span></div>
            <div><span>Base URL</span><span id="ticket-base-url">-</span></div>
            <div><span>Validation</span><span id="ticket-handler-error">-</span></div>
            <div><span>Error</span><span id="ticket-error">-</span></div>
          </div>
        </section>

        <section class="card">
          <h2>Gateway</h2>
          <div class="field">
            <label for="gateway-target">Target</label>
            <input id="gateway-target" value="" placeholder="http://public-host:gateway-port" />
          </div>
          <fieldset id="gateway-controls" disabled style="border: 0; padding: 0; margin: 0;">
            <div class="field">
              <label for="gateway-request">Request JSON</label>
              <textarea id="gateway-request" placeholder="Gateway request body"></textarea>
            </div>
            <div class="actions">
              <button class="primary" id="send-gateway" type="button">Send To Gateway</button>
            </div>
          </fieldset>
          <p class="hint" id="gateway-hint">Gateway controls unlock after a ticket finishes running.</p>
        </section>
      </main>

      <section class="output" style="margin-top: 18px;">
        <h2>Gateway Output</h2>
        <textarea id="gateway-output" readonly placeholder="Gateway response will appear here"></textarea>
      </section>
    </div>

    <script>
      const state = {
        tickets: [],
        selectedTicketId: "",
        defaultRequirement: "",
        gatewayRequestTemplate: "",
        preferredGatewayTarget: "",
      };

      const ticketHistory = document.getElementById("ticket-history");
      const ticketRequirement = document.getElementById("ticket-requirement");
      const gatewayRequest = document.getElementById("gateway-request");
      const gatewayOutput = document.getElementById("gateway-output");
      const gatewayTarget = document.getElementById("gateway-target");
      const gatewayHint = document.getElementById("gateway-hint");
      const gatewayControls = document.getElementById("gateway-controls");
      const submitButton = document.getElementById("submit-ticket");
      const sendGatewayButton = document.getElementById("send-gateway");
      const refreshButton = document.getElementById("reload-state");
      let loadStateInFlight = false;
      let targetManuallyEdited = false;

      function setText(id, value) {
        document.getElementById(id).textContent = value || "-";
      }

      function selectedTicket() {
        return state.tickets.find((ticket) => ticket.ticket_id === state.selectedTicketId) || null;
      }

      function applyTicket(ticket) {
        setText("ticket-id", ticket?.ticket_id || "-");
        setText("rule-id", ticket?.rule_id || "-");
        setText("ticket-status", ticket?.status || "-");
        setText("ticket-runtime", ticket?.gateway_runtime || "-");
        setText("ticket-handler-mode", ticket?.handler_mode || "-");
        setText("ticket-base-url", ticket?.base_url || "-");
        setText("ticket-handler-error", ticket?.handler_validation_error || "-");
        setText("ticket-error", ticket?.error || "-");

        const runtimeOnline = ticket?.gateway_runtime === "online";
        const enabled = Boolean(ticket && ticket.status === "running" && (runtimeOnline || gatewayTarget.value.trim()));
        gatewayControls.disabled = !enabled;
        const preferredTarget = state.preferredGatewayTarget || ticket?.base_url || "";
        if (!targetManuallyEdited || !gatewayTarget.value.trim()) {
          gatewayTarget.value = preferredTarget;
        }
        gatewayHint.textContent = enabled
          ? "Gateway controls are active."
          : (ticket?.status === "running" && !runtimeOnline
            ? "Ticket is loaded, but the gateway is offline until a new run starts or you set a reachable target."
            : "Gateway controls unlock after a ticket finishes running.");
      }

      function renderHistory() {
        const current = state.selectedTicketId;
        ticketHistory.innerHTML = "";

        const empty = document.createElement("option");
        empty.value = "";
        empty.textContent = state.tickets.length ? "Select a ticket" : "No tickets yet";
        ticketHistory.appendChild(empty);

        state.tickets.forEach((ticket) => {
          const option = document.createElement("option");
          option.value = ticket.ticket_id;
          option.textContent = `${ticket.ticket_id} • ${ticket.status}`;
          ticketHistory.appendChild(option);
        });

        const lastTicket = state.tickets.length ? state.tickets[state.tickets.length - 1] : null;
        ticketHistory.value = current && state.tickets.some((ticket) => ticket.ticket_id === current)
          ? current
          : (lastTicket ? lastTicket.ticket_id : "");
        state.selectedTicketId = ticketHistory.value;
        applyTicket(selectedTicket());
      }

      async function loadState() {
        if (loadStateInFlight) {
          return;
        }
        loadStateInFlight = true;
        refreshButton.disabled = true;
        refreshButton.textContent = "Refreshing...";
        try {
          const response = await fetch("/api/ui/state");
          if (!response.ok) {
            throw new Error(`Failed to load UI state: ${response.status}`);
          }
          const data = await response.json();
          state.tickets = data.tickets || [];
          state.defaultRequirement = data.default_requirement || "";
          state.gatewayRequestTemplate = data.gateway_request_template || "";
          state.preferredGatewayTarget = data.preferred_gateway_target || "";
          if (!ticketRequirement.value.trim()) {
            ticketRequirement.value = state.defaultRequirement;
          }
          if (!gatewayRequest.value.trim()) {
            gatewayRequest.value = state.gatewayRequestTemplate;
          }
          renderHistory();
        } finally {
          loadStateInFlight = false;
          refreshButton.disabled = false;
          refreshButton.textContent = "Refresh";
        }
      }

      async function submitTicket() {
        const requirement = ticketRequirement.value.trim();
        if (!requirement) {
          gatewayOutput.value = "Requirement cannot be empty.";
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = "Submitting...";
        try {
          const response = await fetch("/api/tickets", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ requirement }),
          });
          const ticket = await response.json();
          if (!response.ok) {
            gatewayOutput.value = ticket.error || "Ticket submission failed.";
            return;
          }

          gatewayOutput.value = JSON.stringify(ticket, null, 2);
          await loadState();
          state.selectedTicketId = ticket.ticket_id;
          ticketHistory.value = ticket.ticket_id;
          applyTicket(selectedTicket());
        } finally {
          submitButton.disabled = false;
          submitButton.textContent = "Submit Ticket";
        }
      }

      async function sendGateway() {
        const ticket = selectedTicket();
        if (!ticket || ticket.status !== "running" || !gatewayTarget.value.trim()) {
          gatewayOutput.value = "Select a running ticket first.";
          return;
        }

        let body;
        try {
          body = JSON.stringify(JSON.parse(gatewayRequest.value), null, 2);
        } catch (error) {
          gatewayOutput.value = `Invalid JSON: ${error.message}`;
          return;
        }

        sendGatewayButton.disabled = true;
        sendGatewayButton.textContent = "Sending...";
        try {
          const response = await fetch(`/api/tickets/${encodeURIComponent(ticket.ticket_id)}/gateway`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              target: gatewayTarget.value.trim(),
              body: JSON.parse(body),
            }),
          });
          const text = await response.text();
          gatewayOutput.value = text;
        } finally {
          sendGatewayButton.disabled = false;
          sendGatewayButton.textContent = "Send To Gateway";
        }
      }

      gatewayTarget.addEventListener("input", () => {
        targetManuallyEdited = true;
        applyTicket(selectedTicket());
      });
      ticketHistory.addEventListener("change", () => {
        state.selectedTicketId = ticketHistory.value;
        targetManuallyEdited = false;
        applyTicket(selectedTicket());
      });
      submitButton.addEventListener("click", () => submitTicket().catch(reportError));
      sendGatewayButton.addEventListener("click", () => sendGateway().catch(reportError));
      refreshButton.addEventListener("click", () => loadState().catch(reportError));

      function reportError(error) {
        gatewayOutput.value = error.message || String(error);
      }

      loadState().catch(reportError);
      setInterval(() => {
        loadState().catch(reportError);
      }, 2000);
    </script>
  </body>
</html>"#;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/ui/state", get(ui_state))
        .route("/api/tickets", post(create_ticket))
        .route("/api/tickets/:ticket_id", get(get_ticket))
        .route("/api/tickets/:ticket_id/gateway", post(gateway_proxy))
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn ui_state(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.service.ui_state())
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

async fn gateway_proxy(
    State(state): State<AppState>,
    Path(ticket_id): Path<String>,
    Json(request): Json<GatewayProxyRequest>,
) -> impl IntoResponse {
    match state
        .service
        .proxy_gateway_request(&ticket_id, &request.target, &request.body)
        .await
    {
        Ok(response) => ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], response)
            .into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}
