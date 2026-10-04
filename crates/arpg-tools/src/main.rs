mod play;

use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "datapack" => datapack(),
        "bestiary" => bestiary(),
        "replay" => replay(args.get(2).expect("usage: replay <file>")),
        "sim" => sim(
            args.get(2).map(|s| s.as_str()).unwrap_or("41"),
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(100u64),
        ),
        "play" => play::play(),
        _ => help(),
    }
}

fn help() {
    println!("arpg-tools — offline engine inspection");
    println!();
    println!("  datapack           reference datapack manifest (hash, counts)");
    println!("  bestiary           all monster species with stats");
    println!("  replay <file>      decode a replay file: header + entries");
    println!("  sim [seed] [ticks] deterministic sim run; prints hashes/metrics");
    println!("  play               interactive console session (in-process, no network)");
}

fn datapack() {
    let data = arpg_data::datapack::compile_reference_datapack();
    let manifest = arpg_data::datapack::DataPackManifest::of(&data);
    println!("schema_version   {}", manifest.data_schema_version);
    println!("content_revision {}", manifest.content_revision);
    println!("content_hash     {}", hex(&manifest.content_hash));
    println!("skills           {}", data.skills.len());
    println!("monsters         {}", data.monsters.len());
    println!("items            {}", data.items.len());
    println!("levels           {}", data.levels.len());
    if arpg_data::datapack::validate(&data).is_ok() {
        println!("validate         ok");
    } else {
        println!("validate         FAILED");
        std::process::exit(1);
    }
}

fn bestiary() {
    let data = arpg_data::datapack::compile_reference_datapack();
    println!(
        "{:<4} {:<16} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "def", "name", "life", "dmg", "speed", "aggro", "xp", "act"
    );
    for (id, m) in &data.monsters {
        println!(
            "{:<4} {:<16} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
            id.0, m.name, m.base_life, m.damage, m.speed_fp, m.aggro_range, m.experience, m.act
        );
    }
}

fn replay(path: &str) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(1);
        }
    };
    match arpg_replay::Replay::decode(&bytes) {
        Ok(replay) => {
            println!("replay_version {}", replay.header.replay_version);
            println!("engine_version  {}", replay.header.engine_version);
            println!("datapack_hash   {}", hex(&replay.header.datapack_hash));
            println!("ruleset_hash    {}", hex(&replay.header.ruleset_hash));
            println!("root_seed       {}", hex(&replay.header.root_seed));
            println!("entries         {}", replay.entries.len());
            for (i, entry) in replay.entries.iter().enumerate() {
                match entry {
                    arpg_replay::ReplayEntry::Command {
                        execute_tick,
                        player,
                        sequence,
                        payload,
                    } => println!(
                        "  [{i}] tick {execute_tick} player {} seq {sequence} {payload:?}",
                        player.0
                    ),
                    arpg_replay::ReplayEntry::Transition(t) => println!("  [{i}] {t:?}"),
                    arpg_replay::ReplayEntry::Admin { tick, payload } => {
                        println!("  [{i}] admin@{tick} {payload:?}")
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("decode failed: {e}");
            std::process::exit(1);
        }
    }
}

fn sim(seed_str: &str, ticks: u64) {
    let mut seed = [0u8; 32];
    let bytes = seed_str.as_bytes();
    seed[..bytes.len().min(32)].copy_from_slice(&bytes[..bytes.len().min(32)]);
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = arpg_sim::GameInstance::new(data, rules, seed);
    inst.add_player(arpg_core::PlayerId(1), arpg_core::WorldPos::new(0, 0));
    inst.register_datapack_skills().expect("datapack skills");
    let metrics = Arc::new(std::sync::Mutex::new(arpg_metrics::Metrics::new()));
    inst.metrics = Some(Arc::clone(&metrics));
    let mut hash = [0u8; 32];
    for _ in 0..ticks {
        hash = inst.tick().state_hash;
    }
    println!("ticks {}", ticks);
    println!("state_hash {}", hex(&hash));
    print!("{}", metrics.lock().unwrap().render());
    for attack in inst.traces.recent_attacks().take(3) {
        println!("{}", attack.render());
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
