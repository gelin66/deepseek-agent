@long-running
# [LONG RUNNING] Opt-in core command acceptance workflows. Run with:
# cargo test -p dse-tui --bin dse-tui --features long-running-tests commands::groups::core::acceptance -- --test-threads=1
Feature: Core command visible surfaces

  Scenario: Core informational commands write visible transcript messages
    Given a DSE core command workspace
    When the user runs the core command "/help links"
    Then the message window should include "用法： /links"
    And the message window should include "别名： dashboard, api"
    When the user runs the core command "/links"
    Then the message window should include "https://platform.deepseek.com"
    When the user runs the core command "/workspace"
    Then the message window should include "Current workspace:"
    When the user runs the core command "/home"
    Then the message window should include "dse 主面板"
    And the message window should include "/links"

  Scenario: Canonical child work reports a visible dispatch request
    Given a DSE core command workspace
    When the user runs the core command "/agent 2 summarize logs"
    Then the message window should include "Opening persistent sub-agent at depth 2"
