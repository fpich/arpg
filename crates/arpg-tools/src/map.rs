//! ASCII map view for the console session (SPEC.md sections 28-31,
//! 98, 190): renders a window of the generated level centered on the
//! player - walls, floors, interactable objects, monsters, ground
//! items and missiles as text characters. Rendering is presentation
//! only: it never mutates the sim.

use arpg_core::{InteractableKind, ObjectId, WorldPos};
use arpg_sim::GameInstance;

pub const VIEW_W: i32 = 61;
pub const VIEW_H: i32 = 21;

fn obj_char(kind: InteractableKind) -> char {
    match kind {
        InteractableKind::Chest => 'C',
        InteractableKind::Barrel => 'b',
        InteractableKind::Urn => 'u',
        InteractableKind::Door => 'D',
        InteractableKind::Shrine => 'S',
        InteractableKind::Well => 'W',
        InteractableKind::Waypoint => '*',
        InteractableKind::QuestObject => 'Q',
        InteractableKind::GenericInteractable => 'o',
    }
}

/// Render the ASCII map window centered on the player position.
pub fn render(inst: &GameInstance, center: WorldPos) -> String {
    let Some(level) = inst.level.as_ref() else {
        return "no level loaded - the map needs a generated world".to_string();
    };
    let cw = level.collision.width as i32;
    let ch = level.collision.height as i32;
    let cx = center.x / 256;
    let cy = center.y / 256;
    let half_w = VIEW_W / 2;
    let half_h = VIEW_H / 2;
    let x0 = (cx - half_w).max(0);
    let y0 = (cy - half_h).max(0);
    let x1 = (x0 + VIEW_W).min(cw);
    let y1 = (y0 + VIEW_H).min(ch);

    let mut out = String::new();
    out.push_str(&format!(
        "level {}  {}x{}  window x{}..{} y{}..{}  you=({},{})\n",
        level.id.0,
        cw,
        ch,
        x0,
        x1 - 1,
        y0,
        y1 - 1,
        cx,
        cy
    ));
    let top = format!("+{}+", "-".repeat((x1 - x0) as usize));
    out.push_str(&top);
    out.push('\n');
    for y in y0..y1 {
        out.push('|');
        for x in x0..x1 {
            let mut cell = if level.collision.is_walkable(x, y) {
                '.'
            } else {
                '#'
            };
            for m in inst.monsters.values() {
                if m.pos.x / 256 == x && m.pos.y / 256 == y {
                    cell = 'm';
                }
            }
            for p in inst.state.players.values() {
                if p.pos.x / 256 == x && p.pos.y / 256 == y {
                    cell = '@';
                }
            }
            if let Some(obj) = level
                .objects
                .iter()
                .find(|o| o.pos.x / 256 == x && o.pos.y / 256 == y)
            {
                cell = obj_char(obj.kind);
            }
            for (_, gpos) in inst.inventory.items_on_ground(level.id) {
                if gpos.x / 256 == x && gpos.y / 256 == y {
                    cell = '$';
                }
            }
            for missile in &inst.missiles {
                if missile.position.x / 256 == x && missile.position.y / 256 == y {
                    cell = '*';
                }
            }
            out.push(cell);
        }
        out.push('|');
        out.push('\n');
    }
    out.push_str(&top);
    out.push_str("\nlegend: @ you  m monster  # wall  . floor");
    out.push_str("  C chest  S shrine  * waypoint/missile  b barrel  $ loot\n");
    out
}

/// Nearest interactable object id within a small radius of the player,
/// so the `interact` command targets something that exists.
pub fn nearest_object(inst: &GameInstance, center: WorldPos) -> Option<ObjectId> {
    let level = inst.level.as_ref()?;
    let px = center.x / 256;
    let py = center.y / 256;
    level
        .objects
        .iter()
        .map(|o| {
            let dx = (o.pos.x / 256 - px).abs();
            let dy = (o.pos.y / 256 - py).abs();
            (dx + dy, o.id)
        })
        .min_by_key(|(d, id)| (*d, id.0))
        .filter(|(d, _)| *d <= 8)
        .map(|(_, id)| id)
}
