use m18_event_id::identity::normalize_event_id;

fn main() {
    for value in [" DSE.Run_Started ", "dse-tool-finished", "DSE.run.42"] {
        println!("{:?}", normalize_event_id(value));
    }
    for value in [
        ".dse.run",
        "dse..run",
        "dse.run.",
        "agent.run.started",
        "dse.运行.started",
        "dse.run started",
        "dse/run/started",
        "dse.run.started.extra",
    ] {
        println!("{:?}", normalize_event_id(value));
    }
}
