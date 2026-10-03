# arpg

Moteur ARPG headless multijoueur en Rust, inspiré de Diablo II. Serveur autoritaire, simulation déterministe à fixed timestep (25 Hz), data-driven (datapacks remplaçables, rulesets configurables).

Spécification normative : [SPEC.md](SPEC.md).

## Workspace

| Crate | Rôle |
|---|---|
| `arpg-core` | IDs, Tick, fixed-point, RNG hiérarchique (ChaCha8 + BLAKE3), state hash |
| `arpg-data` | `GameData` statique (contenu datapack) |
| `arpg-rules` | `GameRules` (ruleset d'une partie) |
| `arpg-sim` | `GameInstance`, scheduler, phases de tick, commandes, résolution de mouvement (M2) |
| `arpg-world` | Collision, A* déterministe, grille spatiale, génération procédurale avec validation BFS (M2) |
| `arpg-ai` | IA (M5) |
| `arpg-protocol` | Messages Protobuf `arpg.v1`, handshake, snapshots (M1) |
| `arpg-server` | Endpoint QUIC loopback, sessions, pont wire↔sim (M1) |
| `arpg-persistence` | Persistance personnages (M7) |
| `arpg-replay` / `arpg-tools` | Replays déterministes, outillage |

## Build & tests

```sh
cargo build
cargo test
```
