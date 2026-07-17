//! Canonical Run API transport projection.
//!
//! This module contains no Agent loop, durable state, Provider transport, or
//! event translation. It accepts the application service's canonical command
//! and event types and only applies HTTP, SSE, and newline framing.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, extract::Request};
use codewhale_app::AgentApplication;
use codewhale_protocol::agent_runtime::{RunId, StoredRuntimeEvent};
use codewhale_protocol::run_api::{
    DEFAULT_RUN_LIST_LIMIT, RUN_API_SCHEMA_VERSION, RunApiError, RunApiErrorCode, RunCommand,
    RunCommandEnvelope, RunCommandResponse, RunCommandResult,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// HTTP framing and access policy for the canonical local Run API.
#[derive(Clone)]
pub struct AppServerOptions {
    pub listen: SocketAddr,
    pub auth_token: Option<String>,
    pub insecure_no_auth: bool,
    pub cors_origins: Vec<String>,
    pub max_body_bytes: usize,
    pub sse_keep_alive: Duration,
}

impl Default for AppServerOptions {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            auth_token: None,
            insecure_no_auth: false,
            cors_origins: Vec::new(),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            sse_keep_alive: DEFAULT_SSE_KEEP_ALIVE,
        }
    }
}

impl std::fmt::Debug for AppServerOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppServerOptions")
            .field("listen", &self.listen)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("insecure_no_auth", &self.insecure_no_auth)
            .field("cors_origins", &self.cors_origins)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("sse_keep_alive", &self.sse_keep_alive)
            .finish()
    }
}

#[derive(Clone)]
struct TransportState {
    application: Arc<AgentApplication>,
    auth_token: Option<Arc<str>>,
    sse_keep_alive: Duration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventQuery {
    #[serde(default)]
    after_sequence: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceListQuery {
    workspace: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostRoute {
    Start,
    Continue,
    Compact,
    Resume,
    Steer,
    Interrupt,
    Cancel,
    ResolveInteraction,
}

/// Serve the canonical local Run API until the listener stops.
pub async fn run(
    application: Arc<AgentApplication>,
    options: AppServerOptions,
) -> std::io::Result<()> {
    let app = router(application, &options)?;
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    axum::serve(listener, app).await
}

/// Build the canonical HTTP router. The application service is the only
/// stateful product dependency retained by the transport.
pub fn router(
    application: Arc<AgentApplication>,
    options: &AppServerOptions,
) -> std::io::Result<Router> {
    validate_options(options)?;
    let state = TransportState {
        application,
        auth_token: options.auth_token.clone().map(Arc::<str>::from),
        sse_keep_alive: options.sse_keep_alive,
    };
    let protected = Router::new()
        .route("/v1/runs", get(list_root_runs).post(start_run))
        .route("/v1/runs/pending-creations", get(list_pending_creations))
        .route(
            "/v1/runs/pending-creations/{creation_request_id}/recover",
            post(recover_creation),
        )
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/events", get(get_events))
        .route("/v1/runs/{run_id}/continue", post(continue_run))
        .route("/v1/runs/{run_id}/compact", post(compact_run))
        .route("/v1/runs/{run_id}/resume", post(resume_run))
        .route("/v1/runs/{run_id}/steer", post(steer_run))
        .route("/v1/runs/{run_id}/interrupt", post(interrupt_run))
        .route("/v1/runs/{run_id}/cancel", post(cancel_run))
        .route(
            "/v1/runs/{run_id}/interactions/{interaction_id}/resolve",
            post(resolve_interaction),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer_token,
        ));
    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .merge(protected)
        .layer(DefaultBodyLimit::max(options.max_body_bytes));
    if let Some(cors) = cors_layer(&options.cors_origins) {
        app = app.layer(cors);
    }
    Ok(app.with_state(state))
}

/// Process newline-delimited canonical commands. Every input line produces
/// exactly one `RunCommandResponse`; JSON-RPC aliases are not recognized.
pub async fn serve_stdio<R, W>(
    application: Arc<AgentApplication>,
    reader: R,
    mut writer: W,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        let response = match decode_stdio_line(&line) {
            Ok(envelope) => application.execute(envelope).await,
            Err(response) => *response,
        };
        writer.write_all(&encode_stdio_response(&response)?).await?;
        writer.flush().await?;
    }
    Ok(())
}

/// Bind the canonical newline transport to this process's standard streams.
pub async fn run_stdio(application: Arc<AgentApplication>) -> std::io::Result<()> {
    serve_stdio(
        application,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn start_run(
    State(state): State<TransportState>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Start, None, None, payload).await
}

async fn resume_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Resume, Some(run_id), None, payload).await
}

async fn continue_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Continue, Some(run_id), None, payload).await
}

async fn compact_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Compact, Some(run_id), None, payload).await
}

async fn steer_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Steer, Some(run_id), None, payload).await
}

async fn interrupt_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Interrupt, Some(run_id), None, payload).await
}

async fn cancel_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Cancel, Some(run_id), None, payload).await
}

async fn resolve_interaction(
    State(state): State<TransportState>,
    Path((run_id, interaction_id)): Path<(String, String)>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(
        &state,
        PostRoute::ResolveInteraction,
        Some(run_id),
        Some(interaction_id),
        payload,
    )
    .await
}

async fn execute_post(
    state: &TransportState,
    route: PostRoute,
    path_run_id: Option<String>,
    path_interaction_id: Option<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let envelope = match payload {
        Ok(Json(envelope)) => envelope,
        Err(error) => return json_rejection_response(error),
    };
    if let Err(message) = validate_post_envelope(
        route,
        path_run_id.as_deref(),
        path_interaction_id.as_deref(),
        &envelope,
    ) {
        let run_id = path_run_id.map(RunId::from);
        return command_response(invalid_response(&envelope.request_id, message, run_id));
    }
    command_response(state.application.execute(envelope).await)
}

async fn get_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let envelope = generated_envelope(
        request_id(&headers, "get", &run_id),
        RunCommand::Get {
            run_id: RunId::from(run_id),
        },
    );
    command_response(state.application.execute(envelope).await)
}

async fn list_root_runs(
    State(state): State<TransportState>,
    query: Result<Query<WorkspaceListQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return command_response_with_status(
                error.status(),
                invalid_response(
                    request_id(&headers, "list", "roots"),
                    error.body_text(),
                    None,
                ),
            );
        }
    };
    let envelope = generated_envelope(
        request_id(&headers, "list", "roots"),
        RunCommand::ListRoots {
            workspace: query.workspace,
            limit: query.limit.unwrap_or(DEFAULT_RUN_LIST_LIMIT),
        },
    );
    command_response(state.application.execute(envelope).await)
}

async fn list_pending_creations(
    State(state): State<TransportState>,
    query: Result<Query<WorkspaceListQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return command_response_with_status(
                error.status(),
                invalid_response(
                    request_id(&headers, "list", "pending-creations"),
                    error.body_text(),
                    None,
                ),
            );
        }
    };
    let envelope = generated_envelope(
        request_id(&headers, "list", "pending-creations"),
        RunCommand::ListPendingCreations {
            workspace: query.workspace,
            limit: query.limit.unwrap_or(DEFAULT_RUN_LIST_LIMIT),
        },
    );
    command_response(state.application.execute(envelope).await)
}

async fn recover_creation(
    State(state): State<TransportState>,
    Path(path_creation_request_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let envelope = match payload {
        Ok(Json(envelope)) => envelope,
        Err(error) => return json_rejection_response(error),
    };
    let RunCommand::RecoverCreation {
        creation_request_id,
    } = &envelope.command
    else {
        return command_response(invalid_response(
            &envelope.request_id,
            "command kind does not match the recover creation route",
            None,
        ));
    };
    if creation_request_id != &path_creation_request_id {
        return command_response(invalid_response(
            &envelope.request_id,
            format!(
                "path creation request id {path_creation_request_id} does not match command creation request id {creation_request_id}"
            ),
            None,
        ));
    }
    command_response(state.application.execute(envelope).await)
}

async fn get_events(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    query: Result<Query<EventQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return command_response_with_status(
                error.status(),
                invalid_response(
                    request_id(&headers, "events", &run_id),
                    error.body_text(),
                    Some(RunId::from(run_id)),
                ),
            );
        }
    };
    let run_id = RunId::from(run_id);
    if !accepts_sse(&headers) {
        let envelope = generated_envelope(
            request_id(&headers, "events", &run_id.0),
            RunCommand::Events {
                run_id,
                after_sequence: query.after_sequence,
            },
        );
        return command_response(state.application.execute(envelope).await);
    }

    let request_id = request_id(&headers, "events", &run_id.0);
    let first = state
        .application
        .execute(generated_envelope(
            request_id.clone(),
            RunCommand::Events {
                run_id: run_id.clone(),
                after_sequence: query.after_sequence,
            },
        ))
        .await;
    let events = match sse_preflight(first, &run_id) {
        Ok(events) => events,
        Err(response) => return command_response(*response),
    };

    let stream = canonical_event_stream(
        state.application.clone(),
        run_id,
        SseCursor::new(query.after_sequence, events),
    );
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(state.sse_keep_alive)
                .text("keep-alive"),
        )
        .into_response()
}

fn canonical_event_stream(
    application: Arc<AgentApplication>,
    run_id: RunId,
    cursor: SseCursor,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    struct StreamState {
        application: Arc<AgentApplication>,
        run_id: RunId,
        cursor: SseCursor,
    }

    stream::unfold(
        StreamState {
            application,
            run_id,
            cursor,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.cursor.next() {
                    return Some((Ok(canonical_sse_event(&event)), state));
                }
                if state.cursor.finished {
                    return None;
                }
                match state
                    .application
                    .wait_events(&state.run_id, state.cursor.after_sequence)
                    .await
                {
                    RunCommandResult::Events { events, .. } if events.is_empty() => return None,
                    RunCommandResult::Events { events, .. } => {
                        state.cursor.extend(events);
                    }
                    RunCommandResult::Error { .. } => return None,
                    _ => return None,
                }
            }
        },
    )
}

struct SseCursor {
    after_sequence: u64,
    pending: VecDeque<StoredRuntimeEvent>,
    finished: bool,
}

impl SseCursor {
    fn new(after_sequence: u64, events: Vec<StoredRuntimeEvent>) -> Self {
        Self {
            after_sequence,
            pending: VecDeque::from(events),
            finished: false,
        }
    }

    fn extend(&mut self, events: Vec<StoredRuntimeEvent>) {
        self.pending.extend(events);
    }
}

impl Iterator for SseCursor {
    type Item = StoredRuntimeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let event = self.pending.pop_front()?;
        self.after_sequence = event.sequence;
        self.finished = event.event.is_terminal();
        Some(event)
    }
}

fn sse_preflight(
    response: RunCommandResponse,
    run_id: &RunId,
) -> Result<Vec<StoredRuntimeEvent>, Box<RunCommandResponse>> {
    let RunCommandResponse {
        schema_version,
        request_id,
        result,
    } = response;
    match result {
        RunCommandResult::Events { events, .. } => Ok(events),
        RunCommandResult::Error { error } => Err(Box::new(RunCommandResponse {
            schema_version,
            request_id,
            result: RunCommandResult::Error { error },
        })),
        _ => Err(Box::new(invalid_response(
            request_id,
            "application returned a non-event result for event polling",
            Some(run_id.clone()),
        ))),
    }
}

fn canonical_sse_event(event: &StoredRuntimeEvent) -> SseEvent {
    let data =
        serde_json::to_string(event).expect("StoredRuntimeEvent serialization is infallible");
    SseEvent::default()
        .id(event.sequence.to_string())
        .data(data)
}

fn validate_post_envelope(
    route: PostRoute,
    path_run_id: Option<&str>,
    path_interaction_id: Option<&str>,
    envelope: &RunCommandEnvelope,
) -> Result<(), String> {
    let body_run_id = match (&route, &envelope.command) {
        (PostRoute::Start, RunCommand::Start(_)) => return Ok(()),
        (PostRoute::Continue, RunCommand::Continue(command)) => &command.run_id,
        (PostRoute::Compact, RunCommand::Compact(command)) => &command.run_id,
        (PostRoute::Resume, RunCommand::Resume { run_id, .. })
        | (PostRoute::Steer, RunCommand::Steer { run_id, .. })
        | (PostRoute::Interrupt, RunCommand::Interrupt { run_id })
        | (PostRoute::Cancel, RunCommand::Cancel { run_id })
        | (PostRoute::ResolveInteraction, RunCommand::ResolveInteraction { run_id, .. }) => run_id,
        _ => {
            return Err(format!(
                "command kind does not match the {} route",
                route_name(route)
            ));
        }
    };
    let expected = path_run_id.expect("run-scoped POST routes always carry a run id");
    if body_run_id.0 != expected {
        return Err(format!(
            "path run id {expected} does not match command run id {body_run_id}"
        ));
    }
    if route == PostRoute::ResolveInteraction {
        let expected = path_interaction_id
            .expect("interaction resolution route always carries an interaction id");
        let RunCommand::ResolveInteraction { interaction_id, .. } = &envelope.command else {
            unreachable!("command kind was validated above")
        };
        if interaction_id.0 != expected {
            return Err(format!(
                "path interaction id {expected} does not match command interaction id {}",
                interaction_id.0
            ));
        }
    }
    Ok(())
}

fn route_name(route: PostRoute) -> &'static str {
    match route {
        PostRoute::Start => "start",
        PostRoute::Continue => "continue",
        PostRoute::Compact => "compact",
        PostRoute::Resume => "resume",
        PostRoute::Steer => "steer",
        PostRoute::Interrupt => "interrupt",
        PostRoute::Cancel => "cancel",
        PostRoute::ResolveInteraction => "resolve interaction",
    }
}

fn generated_envelope(request_id: String, command: RunCommand) -> RunCommandEnvelope {
    RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id,
        command,
    }
}

fn request_id(headers: &HeaderMap, operation: &str, run_id: &str) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("http-{operation}-{run_id}"))
}

fn invalid_response(
    request_id: impl Into<String>,
    message: impl Into<String>,
    run_id: Option<RunId>,
) -> RunCommandResponse {
    RunCommandResponse {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: request_id.into(),
        result: RunCommandResult::Error {
            error: RunApiError {
                code: RunApiErrorCode::InvalidRequest,
                message: message.into().into_boxed_str(),
                run_id,
                terminal: None,
                creation: None,
            },
        },
    }
}

fn command_response(response: RunCommandResponse) -> Response {
    let status = response_status(&response);
    command_response_with_status(status, response)
}

fn command_response_with_status(status: StatusCode, response: RunCommandResponse) -> Response {
    (status, Json(response)).into_response()
}

fn json_rejection_response(error: JsonRejection) -> Response {
    command_response_with_status(
        error.status(),
        invalid_response("", error.body_text(), None),
    )
}

fn response_status(response: &RunCommandResponse) -> StatusCode {
    match &response.result {
        RunCommandResult::Accepted { .. } => StatusCode::ACCEPTED,
        RunCommandResult::Run { .. }
        | RunCommandResult::Runs { .. }
        | RunCommandResult::PendingCreations { .. }
        | RunCommandResult::Events { .. } => StatusCode::OK,
        RunCommandResult::Error { error } => match error.code {
            RunApiErrorCode::InvalidRequest
            | RunApiErrorCode::EventCursorAhead
            | RunApiErrorCode::InvalidInteractionResponse => StatusCode::BAD_REQUEST,
            RunApiErrorCode::RunNotFound => StatusCode::NOT_FOUND,
            RunApiErrorCode::RunAlreadyExists
            | RunApiErrorCode::RunAlreadyRunning
            | RunApiErrorCode::RunNotActive
            | RunApiErrorCode::RunRecoveryRequired
            | RunApiErrorCode::RunTerminal
            | RunApiErrorCode::RunContinuationInvalid
            | RunApiErrorCode::RunEnvironmentMismatch
            | RunApiErrorCode::InteractionNotPending
            | RunApiErrorCode::InteractionMismatch
            | RunApiErrorCode::InteractionAlreadyResolved => StatusCode::CONFLICT,
            RunApiErrorCode::RunStoreFailed => StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}

fn accepts_sse(headers: &HeaderMap) -> bool {
    headers.get_all(ACCEPT).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value.split(',').any(|media_range| {
                media_range
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("text/event-stream"))
            })
        })
    })
}

fn decode_stdio_line(line: &str) -> Result<RunCommandEnvelope, Box<RunCommandResponse>> {
    serde_json::from_str(line).map_err(|error| {
        Box::new(invalid_response(
            "",
            format!("invalid RunCommandEnvelope JSON: {error}"),
            None,
        ))
    })
}

fn encode_stdio_response(response: &RunCommandResponse) -> std::io::Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    encoded.push(b'\n');
    Ok(encoded)
}

fn validate_options(options: &AppServerOptions) -> std::io::Result<()> {
    if options.insecure_no_auth && options.auth_token.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "auth_token and insecure_no_auth are mutually exclusive",
        ));
    }
    if options.insecure_no_auth && !options.listen.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unauthenticated app-server binds are restricted to loopback addresses",
        ));
    }
    if options.auth_token.is_none() && !options.insecure_no_auth {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "auth_token is required unless insecure_no_auth is explicitly enabled",
        ));
    }
    if options
        .auth_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "auth_token must not be empty",
        ));
    }
    if options.max_body_bytes == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "max_body_bytes must be greater than zero",
        ));
    }
    if options.sse_keep_alive.is_zero() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sse_keep_alive must be greater than zero",
        ));
    }
    for origin in &options.cors_origins {
        HeaderValue::from_str(origin).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid CORS origin {origin:?}: {error}"),
            )
        })?;
    }
    Ok(())
}

fn cors_layer(origins: &[String]) -> Option<CorsLayer> {
    let origins = origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .expect("CORS origins were validated before router construction")
        })
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return None;
    }
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([
                AUTHORIZATION,
                ACCEPT,
                CONTENT_TYPE,
                HeaderName::from_static("x-request-id"),
            ]),
    )
}

async fn require_bearer_token(
    State(state): State<TransportState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.auth_token.as_deref() else {
        return next.run(request).await;
    };
    let authorized = has_valid_bearer_token(request.headers(), expected);
    if authorized {
        next.run(request).await
    } else {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        response
    }
}

fn has_valid_bearer_token(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| provided == expected)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::{Body, to_bytes};
    use axum::http::{Request as HttpRequest, Uri, header};
    use codewhale_app::{
        DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
        TransportRetryPolicy,
    };
    use codewhale_protocol::agent_runtime::{
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION, AgentOutcome, CommandId, InteractionId,
        ModelAccounting, ReasoningEffort, RunLimits, RunPurpose, RuntimeEventId, RuntimeEventKind,
        TerminalState, ToolPolicy, UserInteractionResponse,
    };
    use codewhale_protocol::run_api::{
        CompactRunCommand, ContinueRunCommand, PendingCreationKind, RunProductControls, RunView,
        StartRunCommand,
    };
    use serde_json::json;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::sync::{Notify, Semaphore};
    use tokio::task::JoinHandle;
    use tower::ServiceExt;

    use super::*;

    fn envelope(command: RunCommand) -> RunCommandEnvelope {
        static NEXT_REQUEST_ID: AtomicUsize = AtomicUsize::new(1);
        RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: format!(
                "request-{}",
                NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
            ),
            command,
        }
    }

    fn start_command() -> StartRunCommand {
        StartRunCommand {
            input: "修复问题".to_owned(),
            workspace: "/workspace".to_owned(),
            model: Some("deepseek-v4-pro".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(8_192),
            max_api_requests: NonZeroU32::new(8),
            streaming: true,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            controls: RunProductControls::default(),
        }
    }

    fn event(sequence: u64, terminal: bool) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: RunId::from("run-1"),
            parent_run_id: None,
            event_id: RuntimeEventId(format!("event-{sequence}")),
            sequence,
            occurred_at_unix_ms: 123,
            event: if terminal {
                RuntimeEventKind::Terminal {
                    outcome: Box::new(AgentOutcome {
                        run_id: RunId::from("run-1"),
                        parent_run_id: None,
                        terminal: TerminalState::Cancelled,
                        accounting: ModelAccounting::default(),
                        runtime_model_requests: 0,
                        runtime_retries: 0,
                        tool_calls: 0,
                    }),
                }
            } else {
                RuntimeEventKind::SteerQueued {
                    command_id: CommandId::from(format!("command-{sequence}")),
                    content: "继续".to_owned(),
                }
            },
        }
    }

    async fn response_json(response: Response) -> RunCommandResponse {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body");
        serde_json::from_slice(&bytes).expect("canonical response JSON")
    }

    struct DeepSeekFixture {
        root: String,
        requests: Arc<AtomicUsize>,
        request_notify: Arc<Notify>,
        release: Arc<Semaphore>,
        server: JoinHandle<()>,
    }

    impl DeepSeekFixture {
        async fn start() -> Self {
            #[derive(Clone)]
            struct FixtureState {
                requests: Arc<AtomicUsize>,
                request_notify: Arc<Notify>,
                release: Arc<Semaphore>,
            }

            async fn complete(State(state): State<FixtureState>) -> Json<serde_json::Value> {
                state.requests.fetch_add(1, Ordering::AcqRel);
                state.request_notify.notify_waiters();
                let permit = state.release.acquire().await.expect("fixture remains open");
                permit.forget();
                Json(json!({
                    "id": "fixture-response",
                    "model": "deepseek-v4-flash",
                    "choices": [{
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "生产传输完成"}
                    }],
                    "usage": {
                        "prompt_tokens": 11,
                        "completion_tokens": 3,
                        "total_tokens": 14
                    }
                }))
            }

            let requests = Arc::new(AtomicUsize::new(0));
            let request_notify = Arc::new(Notify::new());
            let release = Arc::new(Semaphore::new(0));
            let state = FixtureState {
                requests: requests.clone(),
                request_notify: request_notify.clone(),
                release: release.clone(),
            };
            let fixture = Router::new()
                .route("/v1/chat/completions", post(complete))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind DeepSeek loopback fixture");
            let address = listener.local_addr().expect("fixture address");
            let server = tokio::spawn(async move {
                axum::serve(listener, fixture)
                    .await
                    .expect("serve DeepSeek fixture");
            });
            Self {
                root: format!("http://{address}/v1"),
                requests,
                request_notify,
                release,
                server,
            }
        }

        fn connection(&self) -> DeepSeekConnectionConfig {
            DeepSeekConnectionConfig {
                endpoint: DeepSeekEndpoint::loopback_fixture(&self.root)
                    .expect("loopback endpoint"),
                strict_tools: false,
                response_header_timeout: Duration::from_secs(2),
                stream_idle_timeout: Duration::from_secs(2),
                retry: TransportRetryPolicy::disabled(),
            }
        }

        fn release_one(&self) {
            self.release.add_permits(1);
        }

        async fn wait_requests(&self, expected: usize) {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let notified = self.request_notify.notified();
                    if self.requests.load(Ordering::Acquire) >= expected {
                        return;
                    }
                    notified.await;
                }
            })
            .await
            .expect("fixture receives expected requests");
        }
    }

    impl Drop for DeepSeekFixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    fn production_app(
        state_db: &Path,
        fixture: &DeepSeekFixture,
        with_key: bool,
    ) -> Arc<AgentApplication> {
        let config = ProductionApplicationConfig::official()
            .with_state_db_path(state_db)
            .with_deepseek_connection(fixture.connection())
            .with_composition_build_revision("app-server-contract");
        let config = if with_key {
            config
                .with_api_key("fixture-key")
                .expect("fixture credential")
        } else {
            config
        };
        Arc::new(AgentApplication::production(config).expect("production application"))
    }

    fn production_start(workspace: &Path, input: &str) -> StartRunCommand {
        StartRunCommand {
            input: input.to_owned(),
            workspace: workspace
                .canonicalize()
                .expect("canonical workspace")
                .display()
                .to_string(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(4_096),
            max_api_requests: NonZeroU32::new(4),
            streaming: false,
            tool_policy: ToolPolicy {
                enabled: false,
                ..ToolPolicy::default()
            },
            limits: RunLimits {
                max_depth: 0,
                model_event_idle_ms: Some(5_000),
                wall_time_ms: Some(10_000),
                ..RunLimits::default()
            },
            controls: RunProductControls::default(),
        }
    }

    fn test_options(auth_token: Option<&str>) -> AppServerOptions {
        AppServerOptions {
            auth_token: auth_token.map(str::to_owned),
            insecure_no_auth: auth_token.is_none(),
            ..AppServerOptions::default()
        }
    }

    async fn post_command(
        app: &Router,
        uri: &str,
        envelope: &RunCommandEnvelope,
        token: Option<&str>,
    ) -> (StatusCode, RunCommandResponse) {
        let mut request = HttpRequest::builder()
            .method(Method::POST)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from(
                        serde_json::to_vec(envelope).expect("serialize command"),
                    ))
                    .expect("command request"),
            )
            .await
            .expect("command response");
        let status = response.status();
        (status, response_json(response).await)
    }

    async fn get_command(
        app: &Router,
        uri: &str,
        token: Option<&str>,
    ) -> (StatusCode, RunCommandResponse) {
        let mut request = HttpRequest::builder().uri(uri);
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).expect("GET request"))
            .await
            .expect("GET response");
        let status = response.status();
        (status, response_json(response).await)
    }

    async fn wait_http_terminal(app: &Router, run_id: &RunId, token: Option<&str>) -> RunView {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (_, response) =
                    get_command(app, &format!("/v1/runs/{}", run_id.0), token).await;
                let RunCommandResult::Run { run } = response.result else {
                    panic!("expected run projection")
                };
                if run.terminal.is_some() {
                    return *run;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("run reaches terminal")
    }

    fn run_from_response(response: RunCommandResponse) -> RunView {
        match response.result {
            RunCommandResult::Run { run } => *run,
            other => panic!("expected run result, got {other:?}"),
        }
    }

    fn events_from_response(response: RunCommandResponse) -> Vec<StoredRuntimeEvent> {
        match response.result {
            RunCommandResult::Events { events, .. } => events,
            other => panic!("expected events result, got {other:?}"),
        }
    }

    async fn sse_events(
        app: &Router,
        run_id: &RunId,
        after_sequence: u64,
    ) -> Vec<StoredRuntimeEvent> {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(format!(
                        "/v1/runs/{}/events?after_sequence={after_sequence}",
                        run_id.0
                    ))
                    .header(ACCEPT, "text/event-stream")
                    .body(Body::empty())
                    .expect("SSE request"),
            )
            .await
            .expect("SSE response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/event-stream"))
        );
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("SSE body");
        String::from_utf8(bytes.to_vec())
            .expect("UTF-8 SSE")
            .split("\n\n")
            .filter_map(|frame| {
                frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .map(|data| serde_json::from_str(data).expect("stored event JSON"))
            })
            .collect()
    }

    #[test]
    fn every_post_route_requires_the_exact_canonical_command_and_run_id() {
        let run_id = RunId::from("run-1");
        let cases = [
            (
                PostRoute::Continue,
                RunCommand::Continue(ContinueRunCommand {
                    run_id: run_id.clone(),
                    input: "下一轮".to_owned(),
                    expected_workspace: None,
                }),
            ),
            (
                PostRoute::Resume,
                RunCommand::Resume {
                    run_id: run_id.clone(),
                    expected_workspace: None,
                },
            ),
            (
                PostRoute::Steer,
                RunCommand::Steer {
                    run_id: run_id.clone(),
                    content: "继续".to_owned(),
                },
            ),
            (
                PostRoute::Interrupt,
                RunCommand::Interrupt {
                    run_id: run_id.clone(),
                },
            ),
            (
                PostRoute::Cancel,
                RunCommand::Cancel {
                    run_id: run_id.clone(),
                },
            ),
        ];
        for (route, command) in cases {
            assert!(
                validate_post_envelope(route, Some("run-1"), None, &envelope(command.clone()))
                    .is_ok()
            );
            assert!(
                validate_post_envelope(route, Some("different"), None, &envelope(command)).is_err()
            );
            assert!(
                validate_post_envelope(
                    route,
                    Some("run-1"),
                    None,
                    &envelope(RunCommand::Get {
                        run_id: run_id.clone()
                    })
                )
                .is_err()
            );
        }
        assert!(
            validate_post_envelope(
                PostRoute::Start,
                None,
                None,
                &envelope(RunCommand::Start(start_command()))
            )
            .is_ok()
        );
        assert!(
            validate_post_envelope(
                PostRoute::Start,
                None,
                None,
                &envelope(RunCommand::Resume {
                    run_id: run_id.clone(),
                    expected_workspace: None,
                })
            )
            .is_err()
        );
        let resolve = RunCommand::ResolveInteraction {
            run_id,
            interaction_id: InteractionId::from("interaction-1"),
            response: UserInteractionResponse::Approved,
        };
        assert!(
            validate_post_envelope(
                PostRoute::ResolveInteraction,
                Some("run-1"),
                Some("interaction-1"),
                &envelope(resolve.clone())
            )
            .is_ok()
        );
        assert!(
            validate_post_envelope(
                PostRoute::ResolveInteraction,
                Some("run-1"),
                Some("different"),
                &envelope(resolve)
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn sse_frame_is_only_sequence_id_and_unchanged_stored_event_json() {
        let event = event(9, false);
        let body = to_bytes(
            Sse::new(stream::iter([Ok::<_, Infallible>(canonical_sse_event(
                &event,
            ))]))
            .into_response()
            .into_body(),
            usize::MAX,
        )
        .await
        .expect("read SSE body");
        let expected = format!(
            "id: 9\ndata: {}\n\n",
            serde_json::to_string(&event).expect("serialize canonical event")
        );
        assert_eq!(body.as_ref(), expected.as_bytes());
    }

    #[test]
    fn terminal_event_is_detected_without_transport_status_translation() {
        assert!(!event(1, false).event.is_terminal());
        assert!(event(2, true).event.is_terminal());
    }

    #[test]
    fn reconnect_replays_strictly_after_the_last_delivered_sequence_without_loss() {
        let mut first_connection = SseCursor::new(0, vec![event(1, false), event(2, false)]);
        assert_eq!(first_connection.next().map(|event| event.sequence), Some(1));

        // The client disconnects after sequence 1. AgentApplication/RunStore
        // returns the canonical events strictly after that cursor.
        let resumed_events = vec![event(2, false), event(3, true)];
        let resumed_sequences = SseCursor::new(1, resumed_events)
            .map(|event| event.sequence)
            .collect::<Vec<_>>();
        assert_eq!(resumed_sequences, vec![2, 3]);
    }

    #[test]
    fn sse_cursor_stops_immediately_after_the_canonical_terminal() {
        let mut cursor = SseCursor::new(1, vec![event(2, true), event(3, false)]);
        assert_eq!(cursor.next().map(|event| event.sequence), Some(2));
        assert!(cursor.next().is_none());
        assert_eq!(cursor.after_sequence, 2);
    }

    #[test]
    fn sse_preflight_preserves_the_application_typed_recovery_error() {
        let response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "events-request".to_owned(),
            result: RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::RunRecoveryRequired,
                    message: "resume required".into(),
                    run_id: Some(RunId::from("run-1")),
                    terminal: None,
                    creation: None,
                },
            },
        };
        let error = sse_preflight(response.clone(), &RunId::from("run-1"))
            .expect_err("typed application error must not enter the SSE body");
        assert_eq!(*error, response);
    }

    #[test]
    fn sse_preflight_rejects_a_non_event_application_result_without_fabricating_an_event() {
        let response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "events-request".to_owned(),
            result: RunCommandResult::Accepted {
                run_id: RunId::from("run-1"),
                last_sequence: 7,
            },
        };
        let error = sse_preflight(response, &RunId::from("run-1"))
            .expect_err("only canonical event results may enter an SSE stream");
        assert!(matches!(
            error.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
    }

    #[test]
    fn accepts_sse_in_a_standard_accept_list() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream; charset=utf-8"),
        );
        assert!(accepts_sse(&headers));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        assert!(!accepts_sse(&headers));
    }

    #[test]
    fn typed_application_errors_map_to_stable_http_statuses() {
        let cases = [
            (RunApiErrorCode::InvalidRequest, StatusCode::BAD_REQUEST),
            (RunApiErrorCode::EventCursorAhead, StatusCode::BAD_REQUEST),
            (RunApiErrorCode::RunNotFound, StatusCode::NOT_FOUND),
            (RunApiErrorCode::RunAlreadyExists, StatusCode::CONFLICT),
            (RunApiErrorCode::RunRecoveryRequired, StatusCode::CONFLICT),
            (RunApiErrorCode::RunAlreadyRunning, StatusCode::CONFLICT),
            (RunApiErrorCode::RunNotActive, StatusCode::CONFLICT),
            (RunApiErrorCode::RunTerminal, StatusCode::CONFLICT),
            (
                RunApiErrorCode::RunContinuationInvalid,
                StatusCode::CONFLICT,
            ),
            (
                RunApiErrorCode::RunEnvironmentMismatch,
                StatusCode::CONFLICT,
            ),
            (
                RunApiErrorCode::RunStoreFailed,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (code, expected) in cases {
            let response = RunCommandResponse {
                schema_version: RUN_API_SCHEMA_VERSION,
                request_id: "request-1".to_owned(),
                result: RunCommandResult::Error {
                    error: RunApiError {
                        code,
                        message: "typed".into(),
                        run_id: None,
                        terminal: None,
                        creation: None,
                    },
                },
            };
            assert_eq!(response_status(&response), expected);
        }

        let accepted = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-accepted".to_owned(),
            result: RunCommandResult::Accepted {
                run_id: RunId::from("run-1"),
                last_sequence: 3,
            },
        };
        assert_eq!(response_status(&accepted), StatusCode::ACCEPTED);
    }

    #[test]
    fn stdio_accepts_all_and_only_canonical_run_command_envelopes() {
        let run_id = RunId::from("run-1");
        let commands = [
            RunCommand::Start(start_command()),
            RunCommand::Continue(ContinueRunCommand {
                run_id: run_id.clone(),
                input: "下一轮".to_owned(),
                expected_workspace: None,
            }),
            RunCommand::Compact(CompactRunCommand {
                run_id: run_id.clone(),
                expected_workspace: None,
            }),
            RunCommand::ListRoots {
                workspace: "/workspace".to_owned(),
                limit: 10,
            },
            RunCommand::ListPendingCreations {
                workspace: "/workspace".to_owned(),
                limit: 10,
            },
            RunCommand::RecoverCreation {
                creation_request_id: "creation-1".to_owned(),
            },
            RunCommand::Get {
                run_id: run_id.clone(),
            },
            RunCommand::Events {
                run_id: run_id.clone(),
                after_sequence: 4,
            },
            RunCommand::Resume {
                run_id: run_id.clone(),
                expected_workspace: None,
            },
            RunCommand::Steer {
                run_id: run_id.clone(),
                content: "继续".to_owned(),
            },
            RunCommand::Interrupt {
                run_id: run_id.clone(),
            },
            RunCommand::Cancel {
                run_id: run_id.clone(),
            },
            RunCommand::ResolveInteraction {
                run_id,
                interaction_id: InteractionId::from("interaction-1"),
                response: UserInteractionResponse::Approved,
            },
        ];
        for command in commands {
            let expected = envelope(command);
            let canonical = serde_json::to_string(&expected).expect("serialize command");
            assert_eq!(
                decode_stdio_line(&canonical).expect("decode canonical envelope"),
                expected
            );
        }

        let json_rpc = r#"{"jsonrpc":"2.0","id":1,"method":"prompt","params":{}}"#;
        let error = decode_stdio_line(json_rpc).expect_err("JSON-RPC must not be an alias");
        assert!(matches!(
            error.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn http_creation_recovery_routes_are_static_thin_application_projections() {
        let temp = tempfile::tempdir().expect("temporary creation recovery workspace");
        let fixture = DeepSeekFixture::start().await;
        let state_db = temp.path().join("state.db");
        let application = production_app(&state_db, &fixture, true);
        let app = router(application.clone(), &test_options(None)).expect("Run API router");
        let workspace = temp
            .path()
            .canonicalize()
            .expect("canonical workspace")
            .display()
            .to_string();

        let creation_request_id = "creation-http-recover";
        let mut start = production_start(temp.path(), "中断自动路由创建");
        start.model = None;
        let interrupted_app = app.clone();
        let interrupted_start = RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: creation_request_id.to_owned(),
            command: RunCommand::Start(start),
        };
        let interrupted = tokio::spawn(async move {
            post_command(&interrupted_app, "/v1/runs", &interrupted_start, None).await
        });
        fixture.wait_requests(1).await;
        interrupted.abort();
        assert!(
            interrupted
                .await
                .expect_err("interrupted creation must not return an HTTP response")
                .is_cancelled()
        );

        let uri = format!(
            "/v1/runs/pending-creations?workspace={}&limit=7",
            workspace.replace('/', "%2F")
        );
        let (status, response) = get_command(&app, &uri, None).await;
        assert_eq!(status, StatusCode::OK);
        let RunCommandResult::PendingCreations {
            workspace: listed_workspace,
            creations,
        } = response.result
        else {
            panic!("the static pending-creations route was captured as {{run_id}}")
        };
        assert_eq!(listed_workspace, workspace);
        assert_eq!(creations.len(), 1);
        assert_eq!(creations[0].creation_request_id, creation_request_id);
        assert_eq!(creations[0].kind, PendingCreationKind::Start);
        assert!(creations[0].unknown_billing);

        let recover = envelope(RunCommand::RecoverCreation {
            creation_request_id: creation_request_id.to_owned(),
        });
        let application_response = application.execute(recover.clone()).await;
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/pending-creations/{creation_request_id}/recover"),
            &recover,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(
            response, application_response,
            "HTTP framing must not rewrite the application typed error"
        );
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::RunRecoveryRequired,
                    creation: Some(ref creation),
                    ..
                }
            } if creation.creation_request_id == creation_request_id && creation.unknown_billing
        ));

        let (status, response) = post_command(
            &app,
            "/v1/runs/pending-creations/different-creation/recover",
            &recover,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InvalidRequest,
                    creation: None,
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn http_continuation_and_root_list_use_the_canonical_application_contract() {
        let temp = tempfile::tempdir().expect("temporary continuation workspace");
        let fixture = DeepSeekFixture::start().await;
        // Eight user turns leave a real prefix beyond the four retained turns,
        // so the manual compact command exercises the model-backed path.
        fixture.release.add_permits(9);
        let application = production_app(&temp.path().join("state.db"), &fixture, true);
        let app = router(application, &test_options(None)).expect("canonical router");
        let start = envelope(RunCommand::Start(production_start(
            temp.path(),
            &format!("第一轮 {}", "甲".repeat(20_000)),
        )));
        let (_, response) = post_command(&app, "/v1/runs", &start, None).await;
        let source = run_from_response(response);
        fixture.wait_requests(1).await;
        wait_http_terminal(&app, &source.run_id, None).await;

        let continuation = envelope(RunCommand::Continue(ContinueRunCommand {
            run_id: source.run_id.clone(),
            input: "第二轮".to_owned(),
            expected_workspace: Some(
                temp.path()
                    .canonicalize()
                    .expect("canonical workspace")
                    .display()
                    .to_string(),
            ),
        }));
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/continue", source.run_id.0),
            &continuation,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let mut continued = run_from_response(response);
        assert_ne!(continued.run_id, source.run_id);
        assert_eq!(continued.continued_from_run_id, Some(source.run_id.clone()));
        let first_continued_id = continued.run_id.clone();
        fixture.wait_requests(2).await;
        wait_http_terminal(&app, &continued.run_id, None).await;
        for turn in 3..=8 {
            let previous_run_id = continued.run_id.clone();
            let continuation = envelope(RunCommand::Continue(ContinueRunCommand {
                run_id: previous_run_id.clone(),
                input: format!("第{turn}轮"),
                expected_workspace: Some(
                    temp.path()
                        .canonicalize()
                        .expect("canonical workspace")
                        .display()
                        .to_string(),
                ),
            }));
            let (status, response) = post_command(
                &app,
                &format!("/v1/runs/{}/continue", previous_run_id.0),
                &continuation,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            continued = run_from_response(response);
            assert_eq!(continued.continued_from_run_id, Some(previous_run_id));
            fixture.wait_requests(turn).await;
            wait_http_terminal(&app, &continued.run_id, None).await;
        }

        let compact_command = envelope(RunCommand::Compact(CompactRunCommand {
            run_id: continued.run_id.clone(),
            expected_workspace: Some(
                temp.path()
                    .canonicalize()
                    .expect("canonical workspace")
                    .display()
                    .to_string(),
            ),
        }));
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/compact", continued.run_id.0),
            &compact_command,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let compacted = run_from_response(response);
        assert_eq!(compacted.purpose, RunPurpose::ContextCompaction);
        assert_eq!(
            compacted.continued_from_run_id,
            Some(continued.run_id.clone())
        );
        fixture.wait_requests(9).await;
        let compacted = wait_http_terminal(&app, &compacted.run_id, None).await;
        assert!(matches!(
            compacted.terminal,
            Some(TerminalState::Completed { .. })
        ));

        let workspace = temp
            .path()
            .canonicalize()
            .expect("canonical workspace")
            .display()
            .to_string();
        let uri = format!(
            "/v1/runs?workspace={}&limit=10",
            workspace.replace('/', "%2F")
        );
        let (status, response) = get_command(&app, &uri, None).await;
        assert_eq!(status, StatusCode::OK);
        let RunCommandResult::Runs {
            workspace: listed_workspace,
            runs,
        } = response.result
        else {
            panic!("expected root run list")
        };
        assert_eq!(listed_workspace, workspace);
        assert_eq!(runs.len(), 9);
        assert!(runs.iter().any(|run| {
            run.run_id == first_continued_id
                && run.continued_from_run_id.as_ref() == Some(&source.run_id)
        }));
        assert!(runs.iter().any(|run| {
            run.run_id == compacted.run_id
                && run.purpose == RunPurpose::ContextCompaction
                && run.continued_from_run_id.as_ref() == Some(&continued.run_id)
        }));
    }

    #[test]
    fn stdio_frame_is_exactly_one_response_line_and_preserves_stored_events() {
        let stored = event(8, false);
        let response = RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "events-request".to_owned(),
            result: RunCommandResult::Events {
                run_id: RunId::from("run-1"),
                after_sequence: 7,
                events: vec![stored.clone()],
            },
        };
        let encoded = encode_stdio_response(&response).expect("encode stdio response");
        assert_eq!(encoded.last(), Some(&b'\n'));
        assert_eq!(encoded.iter().filter(|byte| **byte == b'\n').count(), 1);

        let decoded: RunCommandResponse =
            serde_json::from_slice(&encoded[..encoded.len() - 1]).expect("decode response line");
        assert_eq!(decoded, response);
        assert!(matches!(
            decoded.result,
            RunCommandResult::Events { events, .. } if events == vec![stored]
        ));
    }

    #[test]
    fn event_query_defaults_to_zero_and_rejects_unknown_fields() {
        let Query(default_query) = Query::<EventQuery>::try_from_uri(&Uri::from_static("/events"))
            .expect("default cursor");
        assert_eq!(default_query.after_sequence, 0);

        let Query(query) =
            Query::<EventQuery>::try_from_uri(&Uri::from_static("/events?after_sequence=41"))
                .expect("explicit cursor");
        assert_eq!(query.after_sequence, 41);
        assert!(
            Query::<EventQuery>::try_from_uri(&Uri::from_static("/events?since=41")).is_err(),
            "legacy cursor aliases must fail closed"
        );
    }

    #[test]
    fn request_id_uses_the_explicit_header_or_a_canonical_fallback() {
        let mut headers = HeaderMap::new();
        assert_eq!(request_id(&headers, "events", "run-1"), "http-events-run-1");
        headers.insert(
            "x-request-id",
            HeaderValue::from_static("request-from-client"),
        );
        assert_eq!(
            request_id(&headers, "events", "run-1"),
            "request-from-client"
        );
    }

    #[test]
    fn bearer_auth_accepts_only_the_exact_configured_token() {
        let mut headers = HeaderMap::new();
        assert!(!has_valid_bearer_token(&headers, "secret"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic secret"));
        assert!(!has_valid_bearer_token(&headers, "secret"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        assert!(!has_valid_bearer_token(&headers, "secret"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert!(has_valid_bearer_token(&headers, "secret"));
    }

    #[tokio::test]
    async fn cors_allows_only_configured_origins_and_canonical_request_headers() {
        let app = Router::new()
            .route("/probe", get(|| async { StatusCode::NO_CONTENT }))
            .layer(
                cors_layer(&["https://client.example".to_owned()]).expect("configured CORS layer"),
            );
        let preflight = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::OPTIONS)
                    .uri("/probe")
                    .header(header::ORIGIN, "https://client.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type,x-request-id",
                    )
                    .body(Body::empty())
                    .expect("preflight request"),
            )
            .await
            .expect("preflight response");
        assert_eq!(preflight.status(), StatusCode::OK);
        assert_eq!(
            preflight.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("https://client.example"))
        );
        let allowed_headers = preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|value| value.to_str().ok())
            .expect("allowed headers");
        assert!(allowed_headers.contains("x-request-id"));

        let rejected = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/probe")
                    .header(header::ORIGIN, "https://attacker.example")
                    .body(Body::empty())
                    .expect("cross-origin request"),
            )
            .await
            .expect("cross-origin response");
        assert!(
            rejected
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[tokio::test]
    async fn body_limit_returns_a_typed_invalid_request_with_payload_too_large_status() {
        async fn probe(payload: Result<Json<RunCommandEnvelope>, JsonRejection>) -> Response {
            match payload {
                Ok(_) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => json_rejection_response(error),
            }
        }

        let app = Router::new()
            .route("/probe", post(probe))
            .layer(DefaultBodyLimit::max(32));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/probe")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"payload":"{}"}}"#, "x".repeat(64))))
                    .expect("oversized request"),
            )
            .await
            .expect("body-limit response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let response = response_json(response).await;
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn production_http_covers_every_run_command_and_typed_lifecycle_errors() {
        let temp = tempfile::tempdir().expect("temporary app-server workspace");
        let fixture = DeepSeekFixture::start().await;
        let application = production_app(&temp.path().join("state.db"), &fixture, true);
        let app = router(application, &test_options(None)).expect("canonical router");

        let start = envelope(RunCommand::Start(production_start(
            temp.path(),
            "HTTP 生命周期",
        )));
        let (status, response) = post_command(&app, "/v1/runs", &start, None).await;
        assert_eq!(status, StatusCode::OK);
        let run = run_from_response(response);
        fixture.wait_requests(1).await;

        let (status, response) =
            get_command(&app, &format!("/v1/runs/{}", run.run_id.0), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(run_from_response(response).run_id, run.run_id);

        let (status, response) = get_command(
            &app,
            &format!("/v1/runs/{}/events?after_sequence=0", run.run_id.0),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!events_from_response(response).is_empty());

        let (status, response) = get_command(&app, "/v1/runs/missing", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::RunNotFound,
                    ..
                }
            }
        ));

        let resume = envelope(RunCommand::Resume {
            run_id: run.run_id.clone(),
            expected_workspace: None,
        });
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/resume", run.run_id.0),
            &resume,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::RunAlreadyRunning,
                    ..
                }
            }
        ));

        let mut resolve = envelope(RunCommand::ResolveInteraction {
            run_id: run.run_id.clone(),
            interaction_id: InteractionId::from("interaction-missing"),
            response: UserInteractionResponse::Approved,
        });
        resolve.request_id = "request-resolve-missing".to_owned();
        let (status, response) = post_command(
            &app,
            &format!(
                "/v1/runs/{}/interactions/interaction-missing/resolve",
                run.run_id.0
            ),
            &resolve,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InteractionNotPending,
                    ..
                }
            }
        ));

        let steer = envelope(RunCommand::Steer {
            run_id: run.run_id.clone(),
            content: "先检查边界".to_owned(),
        });
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/steer", run.run_id.0),
            &steer,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let steer_sequence = match response.result {
            RunCommandResult::Accepted { last_sequence, .. } => last_sequence,
            other => panic!("expected accepted steer, got {other:?}"),
        };
        let (_, response) = get_command(
            &app,
            &format!(
                "/v1/runs/{}/events?after_sequence={}",
                run.run_id.0,
                steer_sequence.saturating_sub(1)
            ),
            None,
        )
        .await;
        assert!(events_from_response(response).iter().any(|event| {
            event.sequence == steer_sequence
                && matches!(
                    &event.event,
                    RuntimeEventKind::SteerQueued { content, .. } if content == "先检查边界"
                )
        }));
        let mut stop_steered = envelope(RunCommand::Cancel {
            run_id: run.run_id.clone(),
        });
        stop_steered.request_id = "request-stop-steered".to_owned();
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/cancel", run.run_id.0),
            &stop_steered,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(matches!(response.result, RunCommandResult::Accepted { .. }));
        let steered = wait_http_terminal(&app, &run.run_id, None).await;
        assert!(matches!(steered.terminal, Some(TerminalState::Cancelled)));

        let interrupt_run = envelope(RunCommand::Start(production_start(
            temp.path(),
            "HTTP 中断",
        )));
        let (_, response) = post_command(&app, "/v1/runs", &interrupt_run, None).await;
        let interrupt_run = run_from_response(response);
        fixture.wait_requests(2).await;

        let interrupt = envelope(RunCommand::Interrupt {
            run_id: interrupt_run.run_id.clone(),
        });
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/interrupt", interrupt_run.run_id.0),
            &interrupt,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(matches!(response.result, RunCommandResult::Accepted { .. }));
        let interrupted = wait_http_terminal(&app, &interrupt_run.run_id, None).await;
        assert_eq!(interrupted.terminal, Some(TerminalState::Interrupted));

        let mut cancel_terminal = envelope(RunCommand::Cancel {
            run_id: run.run_id.clone(),
        });
        cancel_terminal.request_id = "request-cancel-terminal".to_owned();
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/cancel", run.run_id.0),
            &cancel_terminal,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(matches!(
            response.result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::RunTerminal,
                    ..
                }
            }
        ));

        let second = envelope(RunCommand::Start(production_start(
            temp.path(),
            "HTTP 取消",
        )));
        let (_, response) = post_command(&app, "/v1/runs", &second, None).await;
        let second = run_from_response(response);
        fixture.wait_requests(3).await;
        let cancel = envelope(RunCommand::Cancel {
            run_id: second.run_id.clone(),
        });
        let (status, response) = post_command(
            &app,
            &format!("/v1/runs/{}/cancel", second.run_id.0),
            &cancel,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(matches!(response.result, RunCommandResult::Accepted { .. }));
        let cancelled = wait_http_terminal(&app, &second.run_id, None).await;
        assert_eq!(cancelled.terminal, Some(TerminalState::Cancelled));
    }

    #[tokio::test]
    async fn terminal_replay_sse_reconnect_and_stdio_are_exact_and_need_no_key() {
        let temp = tempfile::tempdir().expect("temporary app-server workspace");
        let state_path = temp.path().join("state.db");
        let fixture = DeepSeekFixture::start().await;
        fixture.release_one();
        let application = production_app(&state_path, &fixture, true);
        let app = router(application.clone(), &test_options(None)).expect("canonical router");
        let start = envelope(RunCommand::Start(production_start(
            temp.path(),
            "终态精确重放",
        )));
        let (_, response) = post_command(&app, "/v1/runs", &start, None).await;
        let run = run_from_response(response);
        fixture.wait_requests(1).await;
        wait_http_terminal(&app, &run.run_id, None).await;
        let (_, response) = get_command(
            &app,
            &format!("/v1/runs/{}/events?after_sequence=0", run.run_id.0),
            None,
        )
        .await;
        let frozen = events_from_response(response);
        assert!(frozen.last().is_some_and(|event| event.event.is_terminal()));
        assert_eq!(sse_events(&app, &run.run_id, 0).await, frozen);
        let cursor = frozen
            .get(1)
            .unwrap_or_else(|| frozen.first().expect("run events"))
            .sequence;
        assert_eq!(
            sse_events(&app, &run.run_id, cursor).await,
            frozen
                .iter()
                .filter(|event| event.sequence > cursor)
                .cloned()
                .collect::<Vec<_>>()
        );
        drop(app);
        drop(application);
        drop(fixture);

        let quiet_fixture = DeepSeekFixture::start().await;
        let replay_application = production_app(&state_path, &quiet_fixture, false);
        let replay_router = router(replay_application.clone(), &test_options(None))
            .expect("credential-free replay router");
        let (_, response) = get_command(
            &replay_router,
            &format!("/v1/runs/{}/events?after_sequence=0", run.run_id.0),
            None,
        )
        .await;
        assert_eq!(events_from_response(response), frozen);
        let resume = envelope(RunCommand::Resume {
            run_id: run.run_id.clone(),
            expected_workspace: None,
        });
        let (status, response) = post_command(
            &replay_router,
            &format!("/v1/runs/{}/resume", run.run_id.0),
            &resume,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(run_from_response(response).terminal.is_some());
        assert_eq!(sse_events(&replay_router, &run.run_id, 0).await, frozen);

        let stdio_commands = [
            envelope(RunCommand::Get {
                run_id: run.run_id.clone(),
            }),
            envelope(RunCommand::Events {
                run_id: run.run_id.clone(),
                after_sequence: 0,
            }),
            envelope(RunCommand::Resume {
                run_id: run.run_id.clone(),
                expected_workspace: None,
            }),
        ];
        let mut input = stdio_commands
            .iter()
            .map(|command| serde_json::to_string(command).expect("stdio command"))
            .collect::<Vec<_>>();
        input.push(r#"{"jsonrpc":"2.0","id":1,"method":"prompt"}"#.to_owned());
        let input = format!("{}\n", input.join("\n"));
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let (reader, writer) = tokio::io::split(server);
        let stdio_app = replay_application.clone();
        let task = tokio::spawn(async move {
            serve_stdio(stdio_app, BufReader::new(reader), writer)
                .await
                .expect("serve canonical stdio")
        });
        client
            .write_all(input.as_bytes())
            .await
            .expect("write stdio commands");
        client.shutdown().await.expect("close stdio input");
        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .await
            .expect("read stdio responses");
        task.await.expect("stdio task");
        let responses = output
            .lines()
            .map(|line| serde_json::from_str::<RunCommandResponse>(line).expect("response line"))
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 4);
        assert!(matches!(responses[0].result, RunCommandResult::Run { .. }));
        assert!(matches!(
            &responses[1].result,
            RunCommandResult::Events { events, .. } if events == &frozen
        ));
        assert!(matches!(responses[2].result, RunCommandResult::Run { .. }));
        assert!(matches!(
            responses[3].result,
            RunCommandResult::Error {
                error: RunApiError {
                    code: RunApiErrorCode::InvalidRequest,
                    ..
                }
            }
        ));
        assert_eq!(quiet_fixture.requests.load(Ordering::Acquire), 0);
    }

    #[test]
    fn source_and_manifest_have_no_legacy_runtime_or_forbidden_dependencies() {
        let source = include_str!("lib.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source before tests");
        for forbidden in [
            "RuntimeBridge",
            "handle_prompt",
            "spawn_engine",
            "RuntimeThreadStore",
            "EngineEvent",
            "monitor_turn",
            "/v1/chat/completions",
            "/thread",
            "/prompt",
            "/tool",
            "/jobs",
            "/mcp/startup",
            "jsonrpc",
        ] {
            assert!(
                !production.contains(forbidden),
                "legacy app-server source survived: {forbidden}"
            );
        }
        let manifest = include_str!("../Cargo.toml");
        for forbidden in [
            "codewhale-core",
            "codewhale-tui",
            "codewhale-state",
            "codewhale-tools",
            "codewhale-agent",
            "codewhale-config",
            "reqwest",
        ] {
            assert!(
                !manifest.contains(forbidden),
                "forbidden direct dependency survived: {forbidden}"
            );
        }
    }

    #[test]
    fn retired_http_mobile_and_chat_bridge_product_paths_are_absent() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        for relative in [
            "crates/tui/src/runtime_api.rs",
            "crates/tui/src/runtime_api/tests.rs",
            "crates/tui/src/runtime_mobile.html",
            "crates/tui/src/remote_setup/mod.rs",
            "deploy/tencent-lighthouse/systemd/codewhale-runtime.service",
            "scripts/tencent-lighthouse/install-services.sh",
            "integrations/bridge-core/package.json",
            "integrations/feishu-bridge/package.json",
            "integrations/telegram-bridge/package.json",
        ] {
            assert!(
                !workspace.join(relative).exists(),
                "retired product path survived: {relative}"
            );
        }
        for relative in ["crates/cli/src/lib.rs", "crates/tui/src/main.rs"] {
            let source = std::fs::read_to_string(workspace.join(relative))
                .expect("read retained command source");
            for forbidden in [
                "serve --http",
                "serve --mobile",
                "RemoteSetup",
                "remote_setup",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "retired command survived in {relative}: {forbidden}"
                );
            }
        }
    }

    #[test]
    fn options_fail_closed_and_debug_redacts_the_token() {
        let options = AppServerOptions::default();
        assert!(validate_options(&options).is_err());

        let options = AppServerOptions {
            auth_token: Some("secret".to_owned()),
            ..options
        };
        assert!(validate_options(&options).is_ok());
        let debug = format!("{options:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("<redacted>"));

        let insecure_non_loopback = AppServerOptions {
            listen: SocketAddr::from(([0, 0, 0, 0], 8787)),
            auth_token: None,
            insecure_no_auth: true,
            ..AppServerOptions::default()
        };
        assert!(validate_options(&insecure_non_loopback).is_err());
    }
}
