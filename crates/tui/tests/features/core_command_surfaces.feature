@long-running
# [LONG RUNNING] Opt-in core command acceptance workflows. Run with:
# cargo test -p codewhale-tui --bin codewhale-tui --features long-running-tests commands::groups::core::acceptance -- --test-threads=1
Feature: Core command visible surfaces

  Scenario: Core informational commands write visible transcript messages
    Given a CodeWhale core command workspace
    When the user runs the core command "/help links"
    Then the message window should include "用法： /links"
    And the message window should include "别名： dashboard, api"
    When the user runs the core command "/links"
    Then the message window should include "https://platform.deepseek.com"
    When the user runs the core command "/workspace"
    Then the message window should include "Current workspace:"
    When the user runs the core command "/home"
    Then the message window should include "codewhale 主面板"
    And the message window should include "/links"

  Scenario: Core state commands report visible changes
    Given a CodeWhale core command workspace
    When the user runs the core command "/model auto"
    Then the message window should include "模型已切换："
    And the message window should include "auto"

  Scenario: Persistent work commands report visible dispatch requests
    Given a CodeWhale core command workspace
    When the user runs the core command "/agent 2 summarize logs"
    Then the message window should include "Opening persistent sub-agent at depth 2"
    When the user runs the core command "/rlm 1 inspect command extraction"
    Then the message window should include "Opening persistent RLM context at depth 1"
    When the user runs the core command "/fleet help"
    Then the message window should include "/fleet status shows live Fleet worker status"
