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
    RUN_API_SCHEMA_VERSION, RunApiError, RunApiErrorCode, RunCommand, RunCommandEnvelope,
    RunCommandResponse, RunCommandResult,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tower_http::cors::{AllowOrigin, CorsLayer};

const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// HTTP framing and access policy for the canonical local Run API.
#[derive(Clone)]
pub struct CanonicalAppServerOptions {
    pub listen: SocketAddr,
    pub auth_token: Option<String>,
    pub insecure_no_auth: bool,
    pub cors_origins: Vec<String>,
    pub max_body_bytes: usize,
    pub sse_keep_alive: Duration,
}

impl Default for CanonicalAppServerOptions {
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

impl std::fmt::Debug for CanonicalAppServerOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CanonicalAppServerOptions")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostRoute {
    Start,
    Resume,
    Steer,
    Interrupt,
    Cancel,
}

/// Serve the canonical local Run API until the listener stops.
pub async fn serve(
    application: Arc<AgentApplication>,
    options: CanonicalAppServerOptions,
) -> std::io::Result<()> {
    let app = router(application, &options)?;
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    axum::serve(listener, app).await
}

/// Build the canonical HTTP router. The application service is the only
/// stateful product dependency retained by the transport.
pub fn router(
    application: Arc<AgentApplication>,
    options: &CanonicalAppServerOptions,
) -> std::io::Result<Router> {
    validate_options(options)?;
    let state = TransportState {
        application,
        auth_token: options.auth_token.clone().map(Arc::<str>::from),
        sse_keep_alive: options.sse_keep_alive,
    };
    let protected = Router::new()
        .route("/v1/runs", post(start_run))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/events", get(get_events))
        .route("/v1/runs/{run_id}/resume", post(resume_run))
        .route("/v1/runs/{run_id}/steer", post(steer_run))
        .route("/v1/runs/{run_id}/interrupt", post(interrupt_run))
        .route("/v1/runs/{run_id}/cancel", post(cancel_run))
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
    execute_post(&state, PostRoute::Start, None, payload).await
}

async fn resume_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Resume, Some(run_id), payload).await
}

async fn steer_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Steer, Some(run_id), payload).await
}

async fn interrupt_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Interrupt, Some(run_id), payload).await
}

async fn cancel_run(
    State(state): State<TransportState>,
    Path(run_id): Path<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    execute_post(&state, PostRoute::Cancel, Some(run_id), payload).await
}

async fn execute_post(
    state: &TransportState,
    route: PostRoute,
    path_run_id: Option<String>,
    payload: Result<Json<RunCommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let envelope = match payload {
        Ok(Json(envelope)) => envelope,
        Err(error) => return json_rejection_response(error),
    };
    if let Err(message) = validate_post_envelope(route, path_run_id.as_deref(), &envelope) {
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
    envelope: &RunCommandEnvelope,
) -> Result<(), String> {
    let body_run_id = match (&route, &envelope.command) {
        (PostRoute::Start, RunCommand::Start(_)) => return Ok(()),
        (PostRoute::Resume, RunCommand::Resume { run_id })
        | (PostRoute::Steer, RunCommand::Steer { run_id, .. })
        | (PostRoute::Interrupt, RunCommand::Interrupt { run_id })
        | (PostRoute::Cancel, RunCommand::Cancel { run_id }) => run_id,
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
    Ok(())
}

fn route_name(route: PostRoute) -> &'static str {
    match route {
        PostRoute::Start => "start",
        PostRoute::Resume => "resume",
        PostRoute::Steer => "steer",
        PostRoute::Interrupt => "interrupt",
        PostRoute::Cancel => "cancel",
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
                message: message.into(),
                run_id,
                terminal: None,
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
        RunCommandResult::Run { .. } | RunCommandResult::Events { .. } => StatusCode::OK,
        RunCommandResult::Error { error } => match error.code {
            RunApiErrorCode::InvalidRequest | RunApiErrorCode::EventCursorAhead => {
                StatusCode::BAD_REQUEST
            }
            RunApiErrorCode::RunNotFound => StatusCode::NOT_FOUND,
            RunApiErrorCode::RunAlreadyExists
            | RunApiErrorCode::RunAlreadyRunning
            | RunApiErrorCode::RunNotActive
            | RunApiErrorCode::RunRecoveryRequired
            | RunApiErrorCode::RunTerminal
            | RunApiErrorCode::RunEnvironmentMismatch => StatusCode::CONFLICT,
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

fn validate_options(options: &CanonicalAppServerOptions) -> std::io::Result<()> {
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

    use axum::body::{Body, to_bytes};
    use axum::http::{Request as HttpRequest, Uri, header};
    use codewhale_protocol::agent_runtime::{
        AgentOutcome, ModelAccounting, ReasoningEffort, RunLimits, RuntimeEventId,
        RuntimeEventKind, TerminalState, ToolPolicy,
    };
    use codewhale_protocol::run_api::{RunProductControls, StartRunCommand};
    use tower::ServiceExt;

    use super::*;

    fn envelope(command: RunCommand) -> RunCommandEnvelope {
        RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: "request-1".to_owned(),
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
            schema_version: 3,
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
                RuntimeEventKind::Steered {
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

    #[test]
    fn every_post_route_requires_the_exact_canonical_command_and_run_id() {
        let run_id = RunId::from("run-1");
        let cases = [
            (
                PostRoute::Resume,
                RunCommand::Resume {
                    run_id: run_id.clone(),
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
                validate_post_envelope(route, Some("run-1"), &envelope(command.clone())).is_ok()
            );
            assert!(validate_post_envelope(route, Some("different"), &envelope(command)).is_err());
            assert!(
                validate_post_envelope(
                    route,
                    Some("run-1"),
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
                &envelope(RunCommand::Start(start_command()))
            )
            .is_ok()
        );
        assert!(
            validate_post_envelope(
                PostRoute::Start,
                None,
                &envelope(RunCommand::Resume { run_id })
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
                    message: "resume required".to_owned(),
                    run_id: Some(RunId::from("run-1")),
                    terminal: None,
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
                        message: "typed".to_owned(),
                        run_id: None,
                        terminal: None,
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
            RunCommand::Get {
                run_id: run_id.clone(),
            },
            RunCommand::Events {
                run_id: run_id.clone(),
                after_sequence: 4,
            },
            RunCommand::Resume {
                run_id: run_id.clone(),
            },
            RunCommand::Steer {
                run_id: run_id.clone(),
                content: "继续".to_owned(),
            },
            RunCommand::Interrupt {
                run_id: run_id.clone(),
            },
            RunCommand::Cancel { run_id },
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

    #[test]
    fn options_fail_closed_and_debug_redacts_the_token() {
        let options = CanonicalAppServerOptions::default();
        assert!(validate_options(&options).is_err());

        let options = CanonicalAppServerOptions {
            auth_token: Some("secret".to_owned()),
            ..options
        };
        assert!(validate_options(&options).is_ok());
        let debug = format!("{options:?}");
        assert!(!debug.contains("secret"));
        assert!(debug.contains("<redacted>"));

        let insecure_non_loopback = CanonicalAppServerOptions {
            listen: SocketAddr::from(([0, 0, 0, 0], 8787)),
            auth_token: None,
            insecure_no_auth: true,
            ..CanonicalAppServerOptions::default()
        };
        assert!(validate_options(&insecure_non_loopback).is_err());
    }
}
