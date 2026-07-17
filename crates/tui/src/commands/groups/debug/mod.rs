//! Debug command area: canonical cost, balance, and change log.

mod balance;
mod change;
mod cost;

use crate::commands::CommandResult;
use crate::commands::traits::{Command, CommandGroup, CommandInfo, FunctionCommand};
use crate::localization::MessageId;
use crate::tui::app::App;

pub struct DebugCommands;

impl CommandGroup for DebugCommands {
    fn commands(&self) -> &'static [Box<dyn Command>] {
        cached_command_list!(vec![
            Box::new(FunctionCommand::new(&COST_INFO, run_cost)),
            Box::new(FunctionCommand::new(&BALANCE_INFO, run_balance)),
            Box::new(FunctionCommand::new(&CHANGE_INFO, run_change)),
        ])
    }
}

static COST_INFO: CommandInfo = CommandInfo {
    name: "cost",
    aliases: &[],
    usage: "/cost",
    description_id: MessageId::CmdCostDescription,
};
static BALANCE_INFO: CommandInfo = CommandInfo {
    name: "balance",
    aliases: &[],
    usage: "/balance",
    description_id: MessageId::CmdBalanceDescription,
};
static CHANGE_INFO: CommandInfo = CommandInfo {
    name: "change",
    aliases: &[],
    usage: "/change [version]",
    description_id: MessageId::CmdChangeDescription,
};
fn run_registered(app: &mut App, name: &str, arg: Option<&str>) -> CommandResult {
    dispatch(app, name, arg).expect("registered debug command should dispatch")
}

fn run_cost(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "cost", arg)
}
fn run_balance(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "balance", arg)
}
fn run_change(app: &mut App, arg: Option<&str>) -> CommandResult {
    run_registered(app, "change", arg)
}
pub(in crate::commands) fn dispatch(
    app: &mut App,
    command: &str,
    arg: Option<&str>,
) -> Option<CommandResult> {
    let result = match command {
        "cost" => cost::cost(app),
        "balance" => balance::balance(app),
        "change" => change::change(app, arg),
        _ => return None,
    };
    Some(result)
}
