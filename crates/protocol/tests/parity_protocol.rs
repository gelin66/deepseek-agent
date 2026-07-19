use codewhale_protocol::{
    AppRequest, EventFrame, ThreadListParams, ThreadRequest, ThreadResumeParams,
    UserInputAnswerEvent, UserInputOptionEvent, UserInputQuestionEvent, UserInputRequestEvent,
};
use serde_json::json;

#[test]
fn thread_resume_params_round_trip() {
    let request = ThreadRequest::Resume(ThreadResumeParams {
        thread_id: "thread-123".to_string(),
        history: None,
        path: None,
        model: Some("deepseek-v4-pro".to_string()),
        model_provider: Some("deepseek".to_string()),
        cwd: None,
        approval_policy: Some("on-request".to_string()),
        sandbox: Some("workspace-write".to_string()),
        config: None,
        base_instructions: Some("base".to_string()),
        developer_instructions: Some("dev".to_string()),
        personality: Some("default".to_string()),
        persist_extended_history: true,
    });

    let encoded = serde_json::to_string(&request).expect("serialize request");
    let decoded: ThreadRequest = serde_json::from_str(&encoded).expect("deserialize request");
    match decoded {
        ThreadRequest::Resume(params) => {
            assert_eq!(params.thread_id, "thread-123");
            assert_eq!(params.model.as_deref(), Some("deepseek-v4-pro"));
            assert!(params.persist_extended_history);
        }
        other => panic!("unexpected request: {other:?}"),
    }
}

#[test]
fn thread_list_params_defaults_are_serializable() {
    let request = ThreadRequest::List(ThreadListParams {
        include_archived: false,
        limit: Some(20),
    });
    let encoded = serde_json::to_string_pretty(&request).expect("serialize list request");
    assert!(encoded.contains("include_archived"));
}

#[test]
fn event_frame_serialization_contains_expected_tag() {
    let frame = EventFrame::TurnComplete {
        turn_id: "turn-1".to_string(),
    };
    let encoded = serde_json::to_string(&frame).expect("serialize frame");
    assert!(encoded.contains("turn_complete"));
}

#[test]
fn user_input_request_event_frame_round_trip() {
    // issue #3102: the new EventFrame::UserInputRequest variant must tag as
    // "user_input_request" and round-trip the full nested question schema,
    // including the allow_free_text / multi_select booleans.
    let frame = EventFrame::UserInputRequest {
        request: UserInputRequestEvent {
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            request_id: "ui-1".to_string(),
            questions: vec![UserInputQuestionEvent {
                header: "Scope".to_string(),
                id: "scope".to_string(),
                question: "Which surfaces?".to_string(),
                options: vec![
                    UserInputOptionEvent {
                        label: "TUI".to_string(),
                        description: "Modal flow".to_string(),
                    },
                    UserInputOptionEvent {
                        label: "All".to_string(),
                        description: "TUI + headless".to_string(),
                    },
                ],
                allow_free_text: true,
                multi_select: true,
            }],
        },
    };

    let encoded = serde_json::to_value(&frame).expect("serialize user input frame");
    assert_eq!(encoded["event"], "user_input_request");
    assert_eq!(encoded["request"]["call_id"], "call-1");
    assert_eq!(encoded["request"]["request_id"], "ui-1");
    assert_eq!(encoded["request"]["questions"][0]["header"], "Scope");
    assert_eq!(encoded["request"]["questions"][0]["allow_free_text"], true);
    assert_eq!(encoded["request"]["questions"][0]["multi_select"], true);
    assert_eq!(
        encoded["request"]["questions"][0]["options"][0]["label"],
        "TUI"
    );

    // Round-trips back through serde.
    let decoded: EventFrame =
        serde_json::from_value(encoded).expect("deserialize user input frame");
    let EventFrame::UserInputRequest { request } = decoded else {
        panic!("expected user_input_request frame after round-trip");
    };
    assert_eq!(request.request_id, "ui-1");
    assert_eq!(request.questions.len(), 1);
    assert!(request.questions[0].allow_free_text);
    assert!(request.questions[0].multi_select);
}

#[test]
fn user_input_request_event_defaults_flags_when_omitted() {
    // Backwards compatibility: omitting allow_free_text/multi_select in the
    // wire JSON must deserialize both to false (matching the TUI's leniency).
    let input = json!({
        "event": "user_input_request",
        "request": {
            "call_id": "c",
            "turn_id": "t",
            "request_id": "r",
            "questions": [{
                "header": "H",
                "id": "i",
                "question": "Q?",
                "options": [
                    { "label": "A", "description": "a" },
                    { "label": "B", "description": "b" }
                ]
            }]
        }
    });
    let decoded: EventFrame = serde_json::from_value(input).expect("deserialize without flags");
    let EventFrame::UserInputRequest { request } = decoded else {
        panic!("expected user_input_request frame");
    };
    assert!(!request.questions[0].allow_free_text);
    assert!(!request.questions[0].multi_select);
}

#[test]
fn submit_user_input_app_request_round_trip() {
    // issue #3102: the headless client→server reply variant must tag as
    // "submit_user_input" and carry the answer list.
    let req = AppRequest::SubmitUserInput {
        request_id: "ui-1".to_string(),
        answers: vec![UserInputAnswerEvent {
            id: "scope".to_string(),
            label: "All".to_string(),
            value: "All".to_string(),
        }],
    };
    let encoded = serde_json::to_string(&req).expect("serialize submit request");
    assert!(encoded.contains("submit_user_input"));
    assert!(encoded.contains("\"request_id\":\"ui-1\""));

    let decoded: AppRequest = serde_json::from_str(&encoded).expect("deserialize submit request");
    let AppRequest::SubmitUserInput {
        request_id,
        answers,
    } = decoded
    else {
        panic!("expected submit_user_input after round-trip");
    };
    assert_eq!(request_id, "ui-1");
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].label, "All");
}
