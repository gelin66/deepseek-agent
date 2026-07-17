use crate::localization::{MessageId, tr};
use crate::tui::app::App;

use super::CommandResult;

/// Show the cost projected from canonical runtime usage events.
pub fn cost(app: &mut App) -> CommandResult {
    let total = app.displayed_session_cost_for_currency(app.cost_currency);
    let report = tr(app.ui_locale, MessageId::CmdCostReport)
        .replace("{cost}", &app.format_cost_amount_precise(total));
    CommandResult::message(report)
}
