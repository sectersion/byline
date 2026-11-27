// byline report — hours per local day per project from stored rows

pub fn run() -> Result<(), String> {
    let (store, _path) = crate::cli::status::open_store()?;
    let rows: Vec<crate::derive::hours::RawHeartbeat> = store
        .conn
        .prepare("SELECT raw FROM heartbeats ORDER BY id")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|raw| crate::derive::hours::RawHeartbeat { raw })
        .collect();
    let agents: Vec<String> = store.conn
        .prepare("SELECT raw FROM agent_heartbeats ORDER BY id")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    print!("{}", crate::report::text::render(&crate::derive::hours::per_day_project_episode(&rows, &agents)));
    Ok(())
}
