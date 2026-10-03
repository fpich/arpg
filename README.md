# arpg

Moteur ARPG headless multijoueur en Rust, inspiré de Diablo II. Serveur autoritaire, simulation déterministe à fixed timestep (25 Hz), data-driven (datapacks remplaçables, rulesets configurables).

Spécification normative : [SPEC.md](SPEC.md).

## Workspace

| Crate | Rôle |
|---|---|
| `arpg-core` | IDs, Tick, fixed-point, RNG hiérarchique (ChaCha8 + BLAKE3), state hash |
| `arpg-data` | `GameData` statique (contenu datapack) |
| `arpg-rules` | `GameRules` (ruleset d'une partie) |
| `arpg-sim` | `GameInstance`, scheduler, phases de tick, commandes |
| `arpg-world` / `arpg-ai` | Monde, IA (M2/M5) |
| `arpg-protocol` | DTO réseau (QUIC/prost, M1) |
| `arpg-server` / `arpg-persistence` | Serveur, persistance (M1/M7) |
| `arpg-replay` / `arpg-tools` | Replays déterministes, outillage |

## Build & tests

```sh
cargo build
cargo test
```
