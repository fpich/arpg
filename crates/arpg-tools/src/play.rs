//! Interactive console session (SPEC.md sections 169, 190-191): an
//! in-process, loopback-only way to drive the engine from a terminal.
//! The game ticks at the canonical 25 tps on a background thread while
//! the prompt reads commands and submits real `CommandEnvelope`s
//! through the scheduler, exactly like a remote client would. Nothing
//! is exposed on the network.

use arpg_core::{PlayerId, SkillId, WorldPos};
use arpg_sim::{
    ClientCommand, CommandEnvelope, GameInstance, InteractIntent, MoveIntent, MovementMode,
    UseItemIntent, UseSkillIntent,
};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PLAYER: PlayerId = PlayerId(1);
const DEFAULT_MONSTER: u32 = 0;
const DEFAULT_SKILL: u32 = 400;

fn fp_to_tiles(v: i32) -> f64 {
    v as f64 / 256.0
}

fn print_status(inst: &GameInstance) {
    let tick = inst.state.tick.0;
    let p = inst.state.players.get(&PLAYER);
    println!("--- tick {tick} ---");
    if let Some(p) = p {
        println!(
            "you  pos ({:.1},{:.1}) life {} mana {} lvl {} xp {}",
            fp_to_tiles(p.pos.x),
            fp_to_tiles(p.pos.y),
            p.life,
            p.mana,
            p.level,
            p.experience
        );
    } else {
        println!("you  (not in game)");
    }
    for m in inst.monsters.values() {
        let name = m
            .definition
            .and_then(|d| inst.data.monsters.get(&d))
            .map(|d| d.name.as_str())
            .unwrap_or("monster");
        println!(
            "mob  #{} {name} pos ({:.1},{:.1}) life {}",
            m.entity.0,
            fp_to_tiles(m.pos.x),
            fp_to_tiles(m.pos.y),
            m.life
        );
    }
}

fn describe_event(e: &arpg_core::GameEvent) -> Option<String> {
    use arpg_core::GameEvent;
    match e {
        GameEvent::DamageApplied {
            target,
            source,
            amount,
        } => Some(format!("{} hit {} for {amount}", source.0, target.0)),
        GameEvent::EntityKilled { target, killer } => {
            Some(format!("entity {} killed by {}", target.0, killer.0))
        }
        GameEvent::HealingApplied { target, amount } => {
            Some(format!("entity {} healed {amount}", target.0))
        }
        GameEvent::PlayerJoined(p) => Some(format!("player {} joined", p.0)),
        GameEvent::PlayerDisconnected(p) => Some(format!("player {} disconnected", p.0)),
        GameEvent::ItemPickedUp(item) => Some(format!("item {} picked up", item.0)),
        GameEvent::ItemDropped(item) => Some(format!("item {} dropped", item.0)),
        GameEvent::EntityOutOfScope { client, entity } => Some(format!(
            "entity {} out of scope for client {}",
            entity.0, client.0
        )),
        _ => None,
    }
}

fn help() {
    println!("commands:");
    println!("  status                    players, monsters, tick");
    println!("  map                       ASCII map centered on you");
    println!("  move <x> <y>             walk to tile coordinates (tiles)");
    println!("  skill [id]                cast a skill (default {DEFAULT_SKILL} = Barbarian Bash)");
    println!(
        "  spawn [def]               spawn a monster from the bestiary (default {DEFAULT_MONSTER})"
    );
    println!("  hit                       dummy attack a monster near you");
    println!("  interact                  interact with the nearest object");
    println!("  potion                    drink a belt potion");
    println!("  events                    toggle event printing");
    println!("  respawn                   revive in town with full life (test helper)");
    println!("  quit                      leave the session");
}

pub fn play() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(Arc::clone(&data), rules, [41u8; 32]);
    inst.register_datapack_skills().expect("skills register");
    inst.set_ai_brain(Box::new(arpg_ai::HfsmBrain::new(
        arpg_ai::AiParams::default(),
    )));
    let (collision, rooms, objects) =
        arpg_world::generate_level(inst.state.seed(), 1).expect("level generation");
    let level = arpg_world::LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision,
        rooms,
        objects,
    };
    inst.level = Some(level);
    inst.add_player(PLAYER, WorldPos::new(4 * 256, 4 * 256));
    for pos in [(10, 4), (14, 5), (18, 4)] {
        inst.spawn_monster_def(
            arpg_core::MonsterDefId(DEFAULT_MONSTER),
            WorldPos::new(pos.0 * 256, pos.1 * 256),
        );
    }
    let game = Arc::new(Mutex::new(inst));
    let running = Arc::new(AtomicBool::new(true));
    let last_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Background tick loop: 25 tps real time (SPEC.md section 25).
    {
        let game = Arc::clone(&game);
        let running = Arc::clone(&running);
        let last_lines = Arc::clone(&last_lines);
        std::thread::spawn(move || {
            let tick_dur = Duration::from_millis(arpg_core::TICK_DURATION_MS as u64);
            loop {
                std::thread::sleep(tick_dur);
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                let mut g = game.lock().unwrap();
                let result = g.tick();
                for e in &result.events {
                    if let Some(line) = describe_event(e) {
                        let mut lg = last_lines.lock().unwrap();
                        lg.push(line);
                    }
                }
            }
        });
    }

    println!("arpg console session - in-process, loopback only, no network");
    println!("the game ticks at 25 tps while you type; type 'help' for commands");
    let stdin = std::io::stdin();
    let mut seq: u32 = 0;
    let mut show_events = true;
    let mut reader = stdin.lock();
    loop {
        print!("arpg> ");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();
        if show_events {
            let drained: Vec<String> = std::mem::take(&mut *last_lines.lock().unwrap());
            if !drained.is_empty() {
                let mut counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for line in drained {
                    *counts.entry(line).or_default() += 1;
                }
                for (line, n) in counts {
                    if n == 1 {
                        println!("[event] {line}");
                    } else {
                        println!("[event] {line} (x{n})");
                    }
                }
            }
        } else {
            last_lines.lock().unwrap().clear();
        }
        let mut g = game.lock().unwrap();
        match cmd {
            "help" => help(),
            "status" => print_status(&g),
            "map" => {
                let center = g
                    .state
                    .players
                    .get(&PLAYER)
                    .map(|p| p.pos)
                    .unwrap_or(WorldPos::ZERO);
                print!("{}", crate::map::render(&g, center));
            }
            "quit" | "exit" => {
                running.store(false, Ordering::Relaxed);
                println!("bye");
                break;
            }
            "events" => {
                show_events = !show_events;
                println!("events {}", if show_events { "on" } else { "off" });
            }
            "move" => {
                let x: i32 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let y: i32 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let y = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(y);
                seq += 1;
                let tick = g.state.tick;
                g.submit_command(CommandEnvelope {
                    sequence: seq,
                    client_tick: tick,
                    player: PLAYER,
                    command: ClientCommand::Move(MoveIntent {
                        direction: WorldPos::new(x * 256, y * 256),
                        movement_mode: MovementMode::Walk,
                        sequence: seq,
                    }),
                });
                println!("moving to ({x},{y}) tiles");
                {
                    let center = WorldPos::new(x * 256, y * 256);
                    print!("{}", crate::map::render(&g, center));
                }
            }
            "skill" => {
                let id: u32 = rest
                    .first()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_SKILL);
                seq += 1;
                let (tick, target) = {
                    let p = g.state.players.get(&PLAYER);
                    (
                        g.state.tick,
                        p.map(|p| WorldPos::new(p.pos.x + 3 * 256, p.pos.y)),
                    )
                };
                g.submit_command(CommandEnvelope {
                    sequence: seq,
                    client_tick: tick,
                    player: PLAYER,
                    command: ClientCommand::UseSkill(UseSkillIntent {
                        skill: SkillId(id),
                        target,
                    }),
                });
                println!("queued skill {id}");
            }
            "spawn" => {
                let def: u32 = rest
                    .first()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_MONSTER);
                let pos = {
                    let p = g.state.players.get(&PLAYER);
                    p.map(|p| WorldPos::new(p.pos.x + 4 * 256, p.pos.y + 256))
                        .unwrap_or(WorldPos::new(4 * 256, 0))
                };
                let entity = g.spawn_monster_def(arpg_core::MonsterDefId(def), pos);
                println!("spawned entity #{} (def {def})", entity.0);
            }
            "interact" => {
                seq += 1;
                let tick = g.state.tick;
                let p_pos = g
                    .state
                    .players
                    .get(&PLAYER)
                    .map(|p| p.pos)
                    .unwrap_or(WorldPos::ZERO);
                let obj_target =
                    crate::map::nearest_object(&g, p_pos).unwrap_or(arpg_core::ObjectId(0));
                g.submit_command(CommandEnvelope {
                    sequence: seq,
                    client_tick: tick,
                    player: PLAYER,
                    command: ClientCommand::Interact(InteractIntent { target: obj_target }),
                });
                println!("interact queued");
            }
            "potion" => {
                seq += 1;
                let tick = g.state.tick;
                g.submit_command(CommandEnvelope {
                    sequence: seq,
                    client_tick: tick,
                    player: PLAYER,
                    command: ClientCommand::UseItem(UseItemIntent {
                        item: arpg_core::ItemId(1),
                    }),
                });
                println!("potion queued");
            }
            "respawn" => {
                let spawn = g
                    .level
                    .as_ref()
                    .and_then(|l| {
                        (1..l.collision.width as i32).find_map(|x| {
                            (1..l.collision.height as i32)
                                .find(|&y| l.collision.is_walkable(x, y))
                                .map(|y| WorldPos::new(x * 256, y * 256))
                        })
                    })
                    .unwrap_or(WorldPos::new(4 * 256, 4 * 256));
                if let Some(p) = g.state.players.get_mut(&PLAYER) {
                    p.life = 100;
                    p.mana = 50;
                    p.pos = spawn;
                }
                if let Some(actor) = g.actors.get_mut(&arpg_core::EntityId(PLAYER.0 as u64)) {
                    actor.lifecycle = arpg_core::Lifecycle::Alive;
                    actor.mode = arpg_core::ActorMode::Neutral;
                    actor.action = None;
                }
                println!("respawned in town");
            }
            other => println!("unknown command '{other}' - try 'help'"),
        }
    }
    running.store(false, Ordering::Relaxed);
}
