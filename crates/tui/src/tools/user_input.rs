//! Tool and types for requesting user input via the TUI.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{Value, json};

#[cfg(test)]
pub use codewhale_protocol::agent_runtime::UserInputOption;
pub use codewhale_protocol::agent_runtime::{
    UserInputAnswer, UserInputQuestion, UserInputRequest,
    UserInteractionResponse as UserInputResponse,
};

pub fn parse_user_input_request(value: &Value) -> Result<UserInputRequest, ToolError> {
    let request: UserInputRequest = serde_json::from_value(value.clone()).map_err(|error| {
        ToolError::invalid_input(format!("Invalid request_user_input payload: {error}"))
    })?;
    request.validate().map_err(ToolError::invalid_input)?;
    Ok(request)
}

pub struct RequestUserInputTool;

#[async_trait]
impl ToolSpec for RequestUserInputTool {
    fn name(&self) -> &'static str {
        "request_user_input"
    }

    fn description(&self) -> &'static str {
        "Ask the user 1-3 short questions and return their selections."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "header": { "type": "string" },
                            "id": { "type": "string" },
                            "question": { "type": "string" },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    },
                                    "required": ["label", "description"]
                                },
                                "minItems": 2,
                                "maxItems": 4
                            },
                            "allow_free_text": {
                                "type": "boolean",
                                "description": "When true, also offer a free-text 'Other' response. Defaults to false.",
                                "default": false
                            },
                            "multi_select": {
                                "type": "boolean",
                                "description": "When true, allow selecting more than one option. Defaults to false.",
                                "default": false
                            }
                        },
                        "required": ["header", "id", "question", "options"]
                    },
                    "minItems": 1,
                    "maxItems": 3
                }
            },
            "required": ["questions"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        Err(ToolError::execution_failed(
            "request_user_input must be handled by the engine",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_request_shape() {
        let request = UserInputRequest {
            questions: vec![UserInputQuestion {
                header: "Pick".to_string(),
                id: "choice".to_string(),
                question: "Which option?".to_string(),
                options: vec![
                    UserInputOption {
                        label: "A".to_string(),
                        description: "Option A".to_string(),
                    },
                    UserInputOption {
                        label: "B".to_string(),
                        description: "Option B".to_string(),
                    },
                ],
                allow_free_text: false,
                multi_select: false,
            }],
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn from_value_accepts_four_options_and_flags() {
        // Mirrors the json!-literal style used in tools/subagent/tests.rs and
        // exercises the schema-loosening from issue #3102: 4 options (was capped
        // at 3) plus the new allow_free_text / multi_select flags.
        let input = json!({
            "questions": [{
                "header": "Scope",
                "id": "scope",
                "question": "Which surfaces should this change affect?",
                "options": [
                    { "label": "TUI", "description": "Visible modal flow only" },
                    { "label": "Headless", "description": "Protocol event only" },
                    { "label": "All surfaces", "description": "TUI and headless" },
                    { "label": "CLI", "description": "Command-line surface" }
                ],
                "allow_free_text": true,
                "multi_select": true
            }]
        });
        let request = parse_user_input_request(&input).expect("4 options + flags parse");
        assert_eq!(request.questions.len(), 1);
        assert_eq!(request.questions[0].options.len(), 4);
        assert!(request.questions[0].allow_free_text);
        assert!(request.questions[0].multi_select);
    }

    #[test]
    fn from_value_defaults_flags_when_omitted() {
        // Optional boolean fields use the canonical protocol defaults.
        let input = json!({
            "questions": [{
                "header": "Pick",
                "id": "choice",
                "question": "Which?",
                "options": [
                    { "label": "A", "description": "a" },
                    { "label": "B", "description": "b" }
                ]
            }]
        });
        let request = parse_user_input_request(&input).expect("legacy payload parses");
        assert!(!request.questions[0].allow_free_text);
        assert!(!request.questions[0].multi_select);
    }

    #[test]
    fn rejects_five_options() {
        let input = json!({
            "questions": [{
                "header": "Pick",
                "id": "choice",
                "question": "Which?",
                "options": [
                    { "label": "A", "description": "a" },
                    { "label": "B", "description": "b" },
                    { "label": "C", "description": "c" },
                    { "label": "D", "description": "d" },
                    { "label": "E", "description": "e" }
                ]
            }]
        });
        let err = parse_user_input_request(&input).expect_err("5 options must fail");
        assert!(err.to_string().contains("2 to 4 options"));
    }

    fn yes_no_question(header: &str, id: &str) -> UserInputQuestion {
        UserInputQuestion {
            header: header.to_string(),
            id: id.to_string(),
            question: "?".to_string(),
            options: vec![
                UserInputOption {
                    label: "A".to_string(),
                    description: "A".to_string(),
                },
                UserInputOption {
                    label: "B".to_string(),
                    description: "B".to_string(),
                },
            ],
            allow_free_text: false,
            multi_select: false,
        }
    }

    #[test]
    fn rejects_too_many_questions() {
        let request = UserInputRequest {
            questions: vec![
                yes_no_question("Q1", "q1"),
                yes_no_question("Q2", "q2"),
                yes_no_question("Q3", "q3"),
                yes_no_question("Q4", "q4"),
            ],
        };
        assert!(request.validate().is_err());
    }
}
